//! Shared input reads from the selected Chromecast Consumer Control collection.

use crate::raw_input::{
    chromecast_button_for_consumer_usage, chromecast_path_matches_target, decode_chromecast_report,
    RemoteButton,
};
use std::collections::BTreeSet;
use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, SyncSender, TrySendError};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use windows::core::{HRESULT, PCWSTR};
use windows::Win32::Devices::HumanInterfaceDevice::{
    HidD_FreePreparsedData, HidD_GetAttributes, HidD_GetPreparsedData, HidP_GetCaps,
    HidP_GetUsages, HidP_Input, HidP_MaxUsageListLength, HIDD_ATTRIBUTES, HIDP_CAPS,
    HIDP_STATUS_SUCCESS, PHIDP_PREPARSED_DATA,
};
use windows::Win32::Foundation::{
    GetLastError, ERROR_IO_PENDING, GENERIC_READ, HANDLE, WAIT_EVENT, WAIT_FAILED, WAIT_OBJECT_0,
    WAIT_TIMEOUT,
};
use windows::Win32::Storage::FileSystem::{
    CreateFileW, ReadFile, FILE_FLAG_OVERLAPPED, FILE_SHARE_READ, FILE_SHARE_WRITE, OPEN_EXISTING,
};
use windows::Win32::System::Threading::{
    CreateEventW, ResetEvent, SetEvent, WaitForMultipleObjects,
};
use windows::Win32::System::IO::{CancelIoEx, GetOverlappedResult, OVERLAPPED};

const CAST_VENDOR: u16 = 0x18d1;
const CAST_PRODUCT: u16 = 0x9450;
const CONSUMER_PAGE: u16 = 0x0c;
const CONSUMER_CONTROL: u16 = 0x01;
const DESCRIPTOR_REPORT_ID: u8 = 1;
const CONSUMER_USAGE_REPORT_ID: u8 = 2;
const REPORT_BYTES: usize = 3;
const MAX_ACTIVE_USAGES: usize = 128;
const QUEUE_CAPACITY: usize = 32;
static WORKER_ACTIVE: AtomicBool = AtomicBool::new(false);
static NEXT_READER_ID: AtomicU64 = AtomicU64::new(1);
static LAST_OPEN_UNAVAILABLE: Mutex<Option<(&'static str, u32)>> = Mutex::new(None);
const FIRST_IDLE_HEARTBEAT: Duration = Duration::from_secs(8);
const SECOND_IDLE_HEARTBEAT: Duration = Duration::from_secs(30);
const IDLE_HEARTBEAT_INTERVAL: Duration = Duration::from_secs(60);

type ButtonReport = (u8, BTreeSet<RemoteButton>);

struct WorkerSlot;

impl WorkerSlot {
    fn acquire() -> Result<Self, &'static str> {
        WORKER_ACTIVE
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .map(|_| Self)
            .map_err(|_| "reader_still_stopping")
    }
}

impl Drop for WorkerSlot {
    fn drop(&mut self) {
        WORKER_ACTIVE.store(false, Ordering::Release);
    }
}

pub(super) struct Reader {
    id: u64,
    stop: Arc<OwnedHandle>,
    reports: Receiver<ButtonReport>,
    finished: Arc<AtomicBool>,
}

impl Reader {
    pub(super) fn open(
        path: &str,
        address: u64,
        wake: impl Fn() + Send + 'static,
    ) -> Result<Option<Self>, &'static str> {
        if !chromecast_path_matches_target(path, address) {
            return Ok(None);
        }
        let id = NEXT_READER_ID.fetch_add(1, Ordering::Relaxed);
        let open_started = Instant::now();
        let worker_slot =
            WorkerSlot::acquire().map_err(|reason| open_failure(id, open_started, reason, 0))?;
        let win32_path = path
            .strip_prefix(r"\??\")
            .map_or_else(|| path.to_owned(), |suffix| format!(r"\\?\{suffix}"));
        let wide: Vec<_> = win32_path.encode_utf16().chain(Some(0)).collect();
        let file = unsafe {
            CreateFileW(
                PCWSTR(wide.as_ptr()),
                GENERIC_READ.0,
                FILE_SHARE_READ | FILE_SHARE_WRITE,
                None,
                OPEN_EXISTING,
                FILE_FLAG_OVERLAPPED,
                None,
            )
        }
        .map_err(|error| {
            open_failure(
                id,
                open_started,
                "device_open_failed",
                error.code().0 as u32,
            )
        })?;
        let file = own(file);
        let Some(descriptor) = Descriptor::read(raw(&file))
            .map_err(|reason| open_failure(id, open_started, reason, 0))?
        else {
            note_open_unavailable(id, open_started, "skipped", "collection_mismatch", 0);
            return Ok(None);
        };

        let io_done = event().map_err(|reason| open_failure(id, open_started, reason, 0))?;
        let stop = Arc::new(event().map_err(|reason| open_failure(id, open_started, reason, 0))?);
        let open_elapsed_ms = open_started.elapsed().as_millis();
        let worker_stop = Arc::clone(&stop);
        let finished = Arc::new(AtomicBool::new(false));
        let worker_finished = Arc::clone(&finished);
        let (sender, reports) = mpsc::sync_channel(QUEUE_CAPACITY);
        std::thread::Builder::new()
            .name("sayall-chromecast-hid".to_owned())
            .spawn(move || {
                if let Ok(mut last) = LAST_OPEN_UNAVAILABLE.lock() {
                    *last = None;
                }
                crate::ble::gatt_note(format!(
                    "raw_input feature=chromecast action=shared_hid_open phase=completed terminal_result=passed reason=selected_consumer_collection reader_id={id} input_report_bytes={REPORT_BYTES} usage_page=0x{CONSUMER_PAGE:04x} usage=0x{CONSUMER_CONTROL:04x} max_active_usages={} elapsed_ms={open_elapsed_ms}",
                    descriptor.usage_limit
                ));
                let started = Instant::now();
                let mut stats = ReadStats { id, ..ReadStats::default() };
                let result = read_loop(&file, &descriptor, &io_done, &worker_stop, sender, &wake, &mut stats);
                // The listener can start the replacement after seeing this flag.
                drop(descriptor);
                drop(file);
                drop(worker_slot);
                let (terminal, reason, error_code) = match result {
                    Ok(reason) => ("stopped", reason, 0),
                    Err(error) => ("failed", error.reason, error.code),
                };
                crate::ble::gatt_note(format!(
                    "raw_input feature=chromecast action=shared_hid_read phase=completed terminal_result={terminal} reason={reason} reader_id={id} elapsed_ms={} error_code=0x{error_code:08x} {stats}",
                    started.elapsed().as_millis()
                ));
                worker_finished.store(true, Ordering::Release);
                wake();
            })
            .map_err(|_| open_failure(id, open_started, "worker_start_failed", 0))?;

        Ok(Some(Self {
            id,
            stop,
            reports,
            finished,
        }))
    }

    pub(super) fn id(&self) -> u64 {
        self.id
    }

    pub(super) fn try_recv(&self) -> Option<ButtonReport> {
        self.reports.try_recv().ok()
    }

    pub(super) fn is_finished(&self) -> bool {
        self.finished.load(Ordering::Acquire)
    }
}

fn open_failure(id: u64, started: Instant, reason: &'static str, error_code: u32) -> &'static str {
    note_open_unavailable(id, started, "failed", reason, error_code);
    reason
}

fn note_open_unavailable(
    id: u64,
    started: Instant,
    terminal: &str,
    reason: &'static str,
    error_code: u32,
) {
    // The binding audit retries unavailable collections. Preserve transitions
    // while suppressing an unchanged failure at every audit tick.
    let Ok(mut last) = LAST_OPEN_UNAVAILABLE.lock() else {
        return;
    };
    if *last == Some((reason, error_code)) {
        return;
    }
    *last = Some((reason, error_code));
    drop(last);
    crate::ble::gatt_note(format!(
        "raw_input feature=chromecast action=shared_hid_open phase=completed terminal_result={terminal} reason={reason} reader_id={id} elapsed_ms={} error_code=0x{error_code:08x}",
        started.elapsed().as_millis()
    ));
}

impl Drop for Reader {
    fn drop(&mut self) {
        // Only the worker waits for cancellation. It keeps OVERLAPPED and the
        // report buffer alive until the kernel acknowledges the final request.
        let _ = unsafe { SetEvent(raw(&self.stop)) };
    }
}

fn own(handle: HANDLE) -> OwnedHandle {
    // Called only for a successfully acquired owned handle from Win32.
    unsafe { OwnedHandle::from_raw_handle(handle.0) }
}

fn raw(handle: &OwnedHandle) -> HANDLE {
    HANDLE(handle.as_raw_handle())
}

fn event() -> Result<OwnedHandle, &'static str> {
    unsafe { CreateEventW(None, true, false, PCWSTR::null()) }
        .map(own)
        .map_err(|_| "event_create_failed")
}

struct Descriptor {
    preparsed: PHIDP_PREPARSED_DATA,
    usage_limit: usize,
}

impl Descriptor {
    fn read(file: HANDLE) -> Result<Option<Self>, &'static str> {
        let mut attributes = HIDD_ATTRIBUTES {
            Size: std::mem::size_of::<HIDD_ATTRIBUTES>() as u32,
            ..HIDD_ATTRIBUTES::default()
        };
        if !unsafe { HidD_GetAttributes(file, &mut attributes) } {
            return Err("attributes_read_failed");
        }
        if attributes.VendorID != CAST_VENDOR || attributes.ProductID != CAST_PRODUCT {
            return Ok(None);
        }

        let mut preparsed = PHIDP_PREPARSED_DATA::default();
        if !unsafe { HidD_GetPreparsedData(file, &mut preparsed) } {
            return Err("preparsed_data_read_failed");
        }
        let mut descriptor = Self {
            preparsed,
            usage_limit: 0,
        };
        let mut caps = HIDP_CAPS::default();
        if unsafe { HidP_GetCaps(preparsed, &mut caps) } != HIDP_STATUS_SUCCESS {
            return Err("descriptor_caps_read_failed");
        }
        if caps.UsagePage != CONSUMER_PAGE
            || caps.Usage != CONSUMER_CONTROL
            || usize::from(caps.InputReportByteLength) != REPORT_BYTES
        {
            return Ok(None);
        }
        descriptor.usage_limit =
            unsafe { HidP_MaxUsageListLength(HidP_Input, Some(CONSUMER_PAGE), preparsed) } as usize;
        if !(1..=MAX_ACTIVE_USAGES).contains(&descriptor.usage_limit) {
            return Err("descriptor_usage_count_invalid");
        }
        Ok(Some(descriptor))
    }

    fn decode(
        &self,
        report: &mut [u8; REPORT_BYTES],
    ) -> Result<BTreeSet<RemoteButton>, &'static str> {
        let mut usages = [0u16; MAX_ACTIVE_USAGES];
        let mut count = self.usage_limit as u32;
        let status = unsafe {
            HidP_GetUsages(
                HidP_Input,
                CONSUMER_PAGE,
                None,
                usages.as_mut_ptr(),
                &mut count,
                self.preparsed,
                report,
            )
        };
        if status != HIDP_STATUS_SUCCESS || count as usize > self.usage_limit {
            return Err("descriptor_usage_decode_failed");
        }
        Ok(usages[..count as usize]
            .iter()
            .copied()
            .filter_map(chromecast_button_for_consumer_usage)
            .collect())
    }
}

impl Drop for Descriptor {
    fn drop(&mut self) {
        let _ = unsafe { HidD_FreePreparsedData(self.preparsed) };
    }
}

fn decode_button_report(
    report: &mut [u8; REPORT_BYTES],
    decode_descriptor: impl FnOnce(
        &mut [u8; REPORT_BYTES],
    ) -> Result<BTreeSet<RemoteButton>, &'static str>,
) -> Result<Option<ButtonReport>, &'static str> {
    let report_id = report[0];
    let buttons = match report_id {
        DESCRIPTOR_REPORT_ID => decode_descriptor(report)?,
        CONSUMER_USAGE_REPORT_ID => {
            decode_chromecast_report(report).map_err(|_| "consumer_usage_decode_failed")?
        }
        _ => return Ok(None),
    };
    Ok(Some((report_id, buttons)))
}

#[derive(Default)]
struct ReadStats {
    id: u64,
    pending_submits: u64,
    synchronous_submits: u64,
    completions: u64,
    decoded: u64,
    unsupported: u64,
    malformed: u64,
    queued: u64,
}

impl std::fmt::Display for ReadStats {
    fn fmt(&self, out: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            out,
            "pending_submits={} synchronous_submits={} kernel_completions={} decoded_reports={} unsupported_reports={} malformed_reports={} queued_reports={}",
            self.pending_submits, self.synchronous_submits, self.completions,
            self.decoded, self.unsupported, self.malformed, self.queued
        )
    }
}

struct ReadFailure {
    reason: &'static str,
    code: u32,
}

impl ReadFailure {
    fn reason(reason: &'static str) -> Self {
        Self { reason, code: 0 }
    }

    fn windows(reason: &'static str, error: windows::core::Error) -> Self {
        Self {
            reason,
            code: error.code().0 as u32,
        }
    }
}

fn next_idle_heartbeat(elapsed: Duration) -> Duration {
    if elapsed < FIRST_IDLE_HEARTBEAT {
        FIRST_IDLE_HEARTBEAT
    } else if elapsed < SECOND_IDLE_HEARTBEAT {
        SECOND_IDLE_HEARTBEAT
    } else {
        let seconds = IDLE_HEARTBEAT_INTERVAL.as_secs();
        Duration::from_secs((elapsed.as_secs() / seconds + 1) * seconds)
    }
}

fn wait_for_pending_read(waits: &[HANDLE; 2], submitted: Instant, stats: &ReadStats) -> WAIT_EVENT {
    let mut heartbeat_at = FIRST_IDLE_HEARTBEAT;
    loop {
        let timeout = heartbeat_at
            .saturating_sub(submitted.elapsed())
            .as_millis()
            .max(1) as u32;
        let result = unsafe { WaitForMultipleObjects(waits, false, timeout) };
        if result != WAIT_TIMEOUT {
            return result;
        }
        let elapsed = submitted.elapsed();
        if elapsed >= heartbeat_at {
            // A wait timeout is only telemetry. The outstanding operation
            // retains the same OVERLAPPED, event and buffer until it completes.
            crate::ble::gatt_note(format!(
                "raw_input feature=chromecast action=shared_hid_read phase=pending reason=still_waiting_for_completion reader_id={} elapsed_ms={} {stats}",
                stats.id,
                elapsed.as_millis()
            ));
            heartbeat_at = next_idle_heartbeat(elapsed);
        }
    }
}

fn read_loop(
    file: &OwnedHandle,
    descriptor: &Descriptor,
    io_done: &OwnedHandle,
    stop: &OwnedHandle,
    sender: SyncSender<ButtonReport>,
    wake: &impl Fn(),
    stats: &mut ReadStats,
) -> Result<&'static str, ReadFailure> {
    let file = raw(file);
    let io_done = raw(io_done);
    let stop = raw(stop);
    let waits = [stop, io_done];
    let id = stats.id;
    let mut logged_pending = false;
    let mut logged_synchronous = false;
    let mut observed_shapes = BTreeSet::new();
    loop {
        if unsafe { WaitForMultipleObjects(&[stop], false, 0) } == WAIT_OBJECT_0 {
            return Ok("stop_requested");
        }
        unsafe { ResetEvent(io_done) }
            .map_err(|error| ReadFailure::windows("read_event_reset_failed", error))?;
        let mut report = [0; REPORT_BYTES];
        let mut operation = OVERLAPPED {
            hEvent: io_done,
            ..OVERLAPPED::default()
        };
        let submitted = Instant::now();
        let pending = match unsafe { ReadFile(file, Some(&mut report), None, Some(&mut operation)) }
        {
            Ok(()) => {
                stats.synchronous_submits += 1;
                false
            }
            Err(error) if error.code() == HRESULT::from_win32(ERROR_IO_PENDING.0) => {
                stats.pending_submits += 1;
                true
            }
            Err(error) => return Err(ReadFailure::windows("read_submit_failed", error)),
        };
        let (logged, mode) = if pending {
            (&mut logged_pending, "pending")
        } else {
            (&mut logged_synchronous, "synchronous")
        };
        if !*logged {
            *logged = true;
            let reason = if stats.pending_submits + stats.synchronous_submits == 1 {
                "initial_read_submitted"
            } else {
                "submission_mode_observed"
            };
            crate::ble::gatt_note(format!(
                "raw_input feature=chromecast action=shared_hid_read phase=started reason={reason} reader_id={id} submit_mode={mode} elapsed_ms={} {stats}",
                submitted.elapsed().as_millis()
            ));
        }
        if pending {
            let result = wait_for_pending_read(&waits, submitted, stats);
            if result.0 != WAIT_OBJECT_0.0 + 1 {
                let wait_error = if result == WAIT_FAILED {
                    unsafe { GetLastError() }.0
                } else {
                    0
                };
                // A cancellation can race with a completed read. Either way the
                // operation must be drained before its stack buffers are released.
                let cancel = unsafe { CancelIoEx(file, Some(&operation)) }
                    .err()
                    .map_or(0, |error| error.code().0 as u32);
                let mut discarded = 0;
                let reap = unsafe { GetOverlappedResult(file, &operation, &mut discarded, true) }
                    .err()
                    .map_or(0, |error| error.code().0 as u32);
                crate::ble::gatt_note(format!(
                    "raw_input feature=chromecast action=shared_hid_read phase=reaped reason=pending_operation_drained reader_id={id} elapsed_ms={} wait_result=0x{:08x} wait_error={wait_error} cancel_hresult=0x{cancel:08x} reap_hresult=0x{reap:08x}",
                    submitted.elapsed().as_millis(), result.0
                ));
                return if result == WAIT_OBJECT_0 {
                    Ok("stop_requested")
                } else {
                    Err(ReadFailure {
                        reason: "read_wait_failed",
                        code: wait_error,
                    })
                };
            }
        }
        let mut bytes = 0;
        unsafe { GetOverlappedResult(file, &operation, &mut bytes, false) }
            .map_err(|error| ReadFailure::windows("read_completion_failed", error))?;
        stats.completions += 1;
        // Observe kernel delivery before interpreting report ID or length. Each
        // shape is logged once per worker without report bytes or device details.
        let report_id = match (bytes, report[0]) {
            (0, _) => "none",
            (_, DESCRIPTOR_REPORT_ID) => "1",
            (_, CONSUMER_USAGE_REPORT_ID) => "2",
            _ => "other",
        };
        let first_completion = observed_shapes.is_empty();
        if observed_shapes.insert((report_id, bytes)) {
            crate::ble::gatt_note(format!(
                "raw_input feature=chromecast action=shared_hid_read phase=kernel_completion first={first_completion} reader_id={id} submit_mode={mode} report_id={report_id} report_bytes={bytes} elapsed_ms={} {stats}",
                submitted.elapsed().as_millis()
            ));
        }
        if bytes as usize != REPORT_BYTES {
            stats.malformed += 1;
            return Err(ReadFailure::reason("read_report_length_invalid"));
        }
        // Unsupported IDs may belong to another control on the same collection.
        // A malformed supported report ends this stream so the listener can clear
        // held state instead of guessing whether the report meant release.
        let report = match decode_button_report(&mut report, |report| descriptor.decode(report)) {
            Ok(Some(report)) => report,
            Ok(None) => {
                stats.unsupported += 1;
                continue;
            }
            Err(reason) => {
                stats.malformed += 1;
                return Err(ReadFailure::reason(reason));
            }
        };
        stats.decoded += 1;
        match sender.try_send(report) {
            Ok(()) => {
                stats.queued += 1;
                wake();
            }
            Err(TrySendError::Disconnected(_)) => return Ok("receiver_disconnected"),
            Err(TrySendError::Full(_)) => return Err(ReadFailure::reason("read_queue_full")),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn idle_heartbeat_coalesces_without_drifting() {
        for (elapsed, next) in [(0, 8), (7, 8), (8, 30), (30, 60), (60, 120), (141, 180)] {
            assert_eq!(
                next_idle_heartbeat(Duration::from_secs(elapsed)),
                Duration::from_secs(next)
            );
        }
    }

    #[test]
    fn consumer_usage_report_preserves_id_for_press_and_release() {
        let no_descriptor =
            |_: &mut [u8; REPORT_BYTES]| panic!("Report 2 must use the consumer usage decoder");
        assert_eq!(
            decode_button_report(&mut [2, 0x41, 0], no_descriptor),
            Ok(Some((2, BTreeSet::from([RemoteButton::Ok]))))
        );
        assert_eq!(
            decode_button_report(&mut [2, 0, 0], no_descriptor),
            Ok(Some((2, BTreeSet::new())))
        );
    }

    #[test]
    fn unrelated_collection_is_rejected_before_open() {
        let unrelated = r"\\?\hid#vid_18d1&pid_9450#not_the_selected_address";
        assert!(Reader::open(unrelated, 0x123456789abc, || {})
            .unwrap()
            .is_none());
    }

    #[test]
    #[ignore = "Requires a present, explicitly selected Chromecast HID collection"]
    fn selected_shared_hid_read_opens_and_reaps_on_drop() {
        let path = std::env::var("SAYALL_TEST_CHROMECAST_HID_PATH").expect("test HID path");
        let address = std::env::var("SAYALL_TEST_CHROMECAST_HID_ADDRESS").expect("test address");
        let address = u64::from_str_radix(&address, 16).expect("hex test address");
        let (wakes, wake_events) = mpsc::channel();
        let reader = Reader::open(&path, address, move || {
            let _ = wakes.send(());
        })
        .expect("shared reader open")
        .expect("selected Consumer Control collection");
        let finished = Arc::clone(&reader.finished);
        // The worker can be at ReadFile or waiting on a pending request here.
        std::thread::sleep(Duration::from_millis(50));
        assert!(!finished.load(Ordering::Acquire));
        drop(reader);

        let deadline = Instant::now() + Duration::from_secs(2);
        while !finished.load(Ordering::Acquire) {
            wake_events
                .recv_timeout(deadline.saturating_duration_since(Instant::now()))
                .expect("reader cancels and reaps the pending request");
        }
        assert!(!WORKER_ACTIVE.load(Ordering::Acquire));
    }
}
