//! Shared input reads from the selected Chromecast Consumer Control collection.

use crate::raw_input::{
    chromecast_button_for_consumer_usage, chromecast_path_matches_target, RemoteButton,
};
use std::collections::BTreeSet;
use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, SyncSender, TrySendError};
use std::sync::Arc;
use windows::core::{HRESULT, PCWSTR};
use windows::Win32::Devices::HumanInterfaceDevice::{
    HidD_FreePreparsedData, HidD_GetAttributes, HidD_GetPreparsedData, HidP_GetCaps,
    HidP_GetUsages, HidP_Input, HidP_MaxUsageListLength, HIDD_ATTRIBUTES, HIDP_CAPS,
    HIDP_STATUS_SUCCESS, PHIDP_PREPARSED_DATA,
};
use windows::Win32::Foundation::{ERROR_IO_PENDING, GENERIC_READ, HANDLE, WAIT_OBJECT_0};
use windows::Win32::Storage::FileSystem::{
    CreateFileW, ReadFile, FILE_FLAG_OVERLAPPED, FILE_SHARE_READ, FILE_SHARE_WRITE, OPEN_EXISTING,
};
use windows::Win32::System::Threading::{
    CreateEventW, ResetEvent, SetEvent, WaitForMultipleObjects, INFINITE,
};
use windows::Win32::System::IO::{CancelIoEx, GetOverlappedResult, OVERLAPPED};

const CAST_VENDOR: u16 = 0x18d1;
const CAST_PRODUCT: u16 = 0x9450;
const CONSUMER_PAGE: u16 = 0x0c;
const CONSUMER_CONTROL: u16 = 0x01;
const REPORT_ID: u8 = 1;
const REPORT_BYTES: usize = 3;
const MAX_ACTIVE_USAGES: usize = 128;
const QUEUE_CAPACITY: usize = 32;
static WORKER_ACTIVE: AtomicBool = AtomicBool::new(false);

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
    stop: Arc<OwnedHandle>,
    reports: Receiver<BTreeSet<RemoteButton>>,
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
        let worker_slot = WorkerSlot::acquire()?;
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
        .map_err(|_| "device_open_failed")?;
        let file = own(file);
        let Some(descriptor) = Descriptor::read(raw(&file))? else {
            return Ok(None);
        };

        let io_done = event()?;
        let stop = Arc::new(event()?);
        let worker_stop = Arc::clone(&stop);
        let finished = Arc::new(AtomicBool::new(false));
        let worker_finished = Arc::clone(&finished);
        let (sender, reports) = mpsc::sync_channel(QUEUE_CAPACITY);
        std::thread::Builder::new()
            .name("sayall-chromecast-hid".to_owned())
            .spawn(move || {
                let result = read_loop(&file, &descriptor, &io_done, &worker_stop, sender, &wake);
                // The listener can start the replacement after seeing this flag.
                drop(descriptor);
                drop(file);
                drop(worker_slot);
                if let Err(reason) = result {
                    crate::ble::gatt_note(format!(
                        "raw_input feature=chromecast action=shared_hid_read phase=completed terminal_result=failed reason={reason}"
                    ));
                }
                worker_finished.store(true, Ordering::Release);
                wake();
            })
            .map_err(|_| "worker_start_failed")?;

        Ok(Some(Self {
            stop,
            reports,
            finished,
        }))
    }

    pub(super) fn try_recv(&self) -> Option<BTreeSet<RemoteButton>> {
        self.reports.try_recv().ok()
    }

    pub(super) fn is_finished(&self) -> bool {
        self.finished.load(Ordering::Acquire)
    }
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

fn read_loop(
    file: &OwnedHandle,
    descriptor: &Descriptor,
    io_done: &OwnedHandle,
    stop: &OwnedHandle,
    sender: SyncSender<BTreeSet<RemoteButton>>,
    wake: &impl Fn(),
) -> Result<(), &'static str> {
    let file = raw(file);
    let io_done = raw(io_done);
    let stop = raw(stop);
    let waits = [stop, io_done];
    let mut first_read = true;
    loop {
        if unsafe { WaitForMultipleObjects(&[stop], false, 0) } == WAIT_OBJECT_0 {
            return Ok(());
        }
        unsafe { ResetEvent(io_done) }.map_err(|_| "read_event_reset_failed")?;
        let mut report = [0; REPORT_BYTES];
        let mut operation = OVERLAPPED {
            hEvent: io_done,
            ..OVERLAPPED::default()
        };
        let pending = match unsafe { ReadFile(file, Some(&mut report), None, Some(&mut operation)) }
        {
            Ok(()) => false,
            Err(error) if error.code() == HRESULT::from_win32(ERROR_IO_PENDING.0) => true,
            Err(_) => return Err("read_submit_failed"),
        };
        if first_read {
            first_read = false;
            crate::ble::gatt_note(
                "raw_input feature=chromecast action=shared_hid_read phase=started reason=initial_read_submitted"
                    .to_owned(),
            );
        }
        if pending {
            let result = unsafe { WaitForMultipleObjects(&waits, false, INFINITE) };
            if result.0 != WAIT_OBJECT_0.0 + 1 {
                // A cancellation can race with a completed read. Either way the
                // operation must be drained before its stack buffers are released.
                let _ = unsafe { CancelIoEx(file, Some(&operation)) };
                let mut discarded = 0;
                let _ = unsafe { GetOverlappedResult(file, &operation, &mut discarded, true) };
                return if result == WAIT_OBJECT_0 {
                    Ok(())
                } else {
                    Err("read_wait_failed")
                };
            }
        }
        let mut bytes = 0;
        unsafe { GetOverlappedResult(file, &operation, &mut bytes, false) }
            .map_err(|_| "read_completion_failed")?;
        if bytes as usize != REPORT_BYTES {
            return Err("read_report_length_invalid");
        }
        if report[0] != REPORT_ID {
            continue;
        }
        let buttons = descriptor.decode(&mut report)?;
        match sender.try_send(buttons) {
            Ok(()) => wake(),
            Err(TrySendError::Disconnected(_)) => return Ok(()),
            Err(TrySendError::Full(_)) => return Err("read_queue_full"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, Instant};

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
