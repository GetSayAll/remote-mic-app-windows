//! 豆包输入法 TSF 身份与激活探针（2026-10-01）：
//! `list` 列出当前会话的 TSF 输入处理器配置并标出豆包一行；
//! `activate-doubao` 用与应用 ime.rs 相同的 STA 纪律试切一次，读回活动配置后
//! 还原原配置（阳性对照：防"返回 S_OK 但没生效"的 MTA 陷阱）。
//!
//! 身份来源：HKLM\SOFTWARE\Microsoft\CTF\TIP\
//! {9D2B2E2B-3C93-4D2F-9D35-6EEB85F0D2B0}\LanguageProfile\0x00000804\
//! {2B4D4B3A-4D4F-4C0A-8E66-7F771A2B9C10}（Description=豆包输入法，Enable=1）。
//! 只读 + 一次显式激活（用完还原）；日志只落 GUID，不含用户路径。

use windows::core::GUID;
use windows::Win32::System::Com::{
    CoCreateInstance, CoInitializeEx, CoUninitialize, CLSCTX_INPROC_SERVER,
    COINIT_APARTMENTTHREADED,
};
use windows::Win32::UI::Input::KeyboardAndMouse::HKL;
use windows::Win32::UI::TextServices::{
    ITfInputProcessorProfileMgr, TF_INPUTPROCESSORPROFILE, TF_IPPMF_FORSESSION,
    TF_IPP_FLAG_ENABLED, TF_PROFILETYPE_INPUTPROCESSOR,
};

const CLSID_TF_INPUT_PROCESSOR_PROFILES: GUID =
    GUID::from_u128(0x33c53a50_f456_4884_b049_85fd643e_cfed);
const GUID_TFCAT_TIP_KEYBOARD: GUID = GUID::from_u128(0x34745c63_b2f0_4784_8b67_5e12c8701a31);
const DOUBAO_CLSID: GUID = GUID::from_u128(0x9d2b2e2b_3c93_4d2f_9d35_6eeb85f0d2b0);
const DOUBAO_PROFILE: GUID = GUID::from_u128(0x2b4d4b3a_4d4f_4c0a_8e66_7f771a2b9c10);
const WETYPE_CLSID: GUID = GUID::from_u128(0x86598fb9_66a2_463e_b9c2_aeb906d477ad);
const LANGID_ZH_CN: u16 = 0x0804;

fn main() {
    let mode = std::env::args().nth(1).unwrap_or_else(|| "list".to_owned());
    let (sender, receiver) = std::sync::mpsc::channel();
    std::thread::Builder::new()
        .name("probe-sta".to_owned())
        .spawn(move || {
            let _ = sender.send(run_sta(&mode));
        })
        .expect("spawn sta thread")
        .join()
        .ok();
    match receiver.recv_timeout(std::time::Duration::from_secs(10)) {
        Ok(Ok(())) => {}
        Ok(Err(error)) => println!("probe failed: {error}"),
        Err(_) => println!("probe timeout"),
    }
}

fn run_sta(mode: &str) -> Result<(), String> {
    unsafe {
        let hr = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
        if hr.is_err() && hr != windows::core::HRESULT(1) {
            return Err(format!("CoInitializeEx(STA) failed: {hr:?}"));
        }
        let result = (|| {
            let manager: ITfInputProcessorProfileMgr = CoCreateInstance(
                &CLSID_TF_INPUT_PROCESSOR_PROFILES,
                None,
                CLSCTX_INPROC_SERVER,
            )
            .map_err(|e| format!("CoCreateInstance(profiles mgr): {e}"))?;

            let original = active_profile_raw(&manager);
            println!("active_before={}", describe(original));
            let rows = enumerate(&manager)?;
            for row in &rows {
                let mark = if row.clsid == DOUBAO_CLSID {
                    " <== doubao"
                } else if row.clsid == WETYPE_CLSID {
                    " (wetype)"
                } else {
                    ""
                };
                println!(
                    "langid=0x{langid:04X} clsid={clsid} guidProfile={guid} flags=0x{flags:08X} enabled={enabled}{mark}",
                    langid = row.langid,
                    clsid = fmt_guid(&row.clsid),
                    guid = fmt_guid(&row.guid_profile),
                    flags = row.flags,
                    enabled = row.flags & TF_IPP_FLAG_ENABLED != 0,
                );
            }

            let doubao = rows.iter().find(|row| row.clsid == DOUBAO_CLSID);
            match doubao {
                Some(row) => println!(
                    "doubao langid=0x{:04X} guidProfile={}",
                    row.langid,
                    fmt_guid(&row.guid_profile)
                ),
                None => {
                    println!("doubao profile not found in session enumeration");
                    return Ok(());
                }
            }
            if mode != "activate-doubao" {
                return Ok(());
            }
            manager
                .ActivateProfile(
                    TF_PROFILETYPE_INPUTPROCESSOR,
                    LANGID_ZH_CN,
                    &DOUBAO_CLSID,
                    &DOUBAO_PROFILE,
                    HKL::default(),
                    TF_IPPMF_FORSESSION,
                )
                .map_err(|e| format!("ActivateProfile(doubao): {e}"))?;
            std::thread::sleep(std::time::Duration::from_millis(120));
            println!(
                "active_after_activate={}",
                describe(active_profile_raw(&manager))
            );

            // 还原探针前的活动配置（探针不留副作用）。
            if let Some((clsid, profile, langid)) = original {
                let _ = manager.ActivateProfile(
                    TF_PROFILETYPE_INPUTPROCESSOR,
                    langid,
                    &clsid,
                    &profile,
                    HKL::default(),
                    TF_IPPMF_FORSESSION,
                );
                std::thread::sleep(std::time::Duration::from_millis(120));
                println!(
                    "active_after_restore={}",
                    describe(active_profile_raw(&manager))
                );
            }
            Ok(())
        })();
        CoUninitialize();
        result
    }
}

struct Row {
    langid: u16,
    clsid: GUID,
    guid_profile: GUID,
    flags: u32,
}

fn enumerate(manager: &ITfInputProcessorProfileMgr) -> Result<Vec<Row>, String> {
    let mut rows = Vec::new();
    let enumerator =
        unsafe { manager.EnumProfiles(LANGID_ZH_CN) }.map_err(|e| format!("EnumProfiles: {e}"))?;
    let mut batch = [TF_INPUTPROCESSORPROFILE::default(); 16];
    loop {
        let mut fetched: u32 = 0;
        if unsafe { enumerator.Next(&mut batch, &mut fetched) }.is_err() || fetched == 0 {
            break;
        }
        for profile in &batch[..fetched as usize] {
            if profile.dwProfileType != TF_PROFILETYPE_INPUTPROCESSOR {
                continue;
            }
            rows.push(Row {
                langid: profile.langid,
                clsid: profile.clsid,
                guid_profile: profile.guidProfile,
                flags: profile.dwFlags,
            });
        }
        if (fetched as usize) < batch.len() {
            break;
        }
    }
    Ok(rows)
}

fn active_profile_raw(manager: &ITfInputProcessorProfileMgr) -> Option<(GUID, GUID, u16)> {
    let mut profile = TF_INPUTPROCESSORPROFILE::default();
    unsafe { manager.GetActiveProfile(&GUID_TFCAT_TIP_KEYBOARD, &mut profile) }.ok()?;
    Some((profile.clsid, profile.guidProfile, profile.langid))
}

fn describe(active: Option<(GUID, GUID, u16)>) -> String {
    match active {
        Some((clsid, guid, langid)) => format!(
            "clsid={} guidProfile={} langid=0x{langid:04X}",
            fmt_guid(&clsid),
            fmt_guid(&guid)
        ),
        None => "<query failed>".to_owned(),
    }
}

fn fmt_guid(guid: &GUID) -> String {
    format!(
        "{{{:08X}-{:04X}-{:04X}-{:02X}{:02X}-{:02X}{:02X}{:02X}{:02X}{:02X}{:02X}}}",
        guid.data1,
        guid.data2,
        guid.data3,
        guid.data4[0],
        guid.data4[1],
        guid.data4[2],
        guid.data4[3],
        guid.data4[4],
        guid.data4[5],
        guid.data4[6],
        guid.data4[7],
    )
}
