//! Non-persistent TSF probe for Chatterfly's modifier-only voice hotkey.
//!
//! Uses only public TSF APIs. It activates Chatterfly for this STA session and
//! asks the TSF keystroke manager whether Ctrl+Win is a registered preserved
//! key, then enumerates public TSF language-bar items. It does not invoke a
//! command, change persistent settings, or access Chatterfly private state.

use windows::core::{Interface, GUID};
use windows::Win32::System::Com::{
    CoCreateInstance, CoInitializeEx, CoUninitialize, CLSCTX_INPROC_SERVER,
    COINIT_APARTMENTTHREADED,
};
use windows::Win32::UI::Input::KeyboardAndMouse::HKL;
use windows::Win32::UI::TextServices::{
    ITfContext, ITfInputProcessorProfileMgr, ITfKeystrokeMgr, ITfLangBarItem, ITfLangBarItemButton,
    ITfLangBarItemMgr, ITfThreadMgr, TF_IPPMF_FORSESSION, TF_LANGBARITEMINFO, TF_PRESERVEDKEY,
    TF_PROFILETYPE_INPUTPROCESSOR,
};

const CLSID_TF_THREAD_MGR: GUID = GUID::from_u128(0x529a9e6b_6587_4f23_ab9e_9c7d683e3c50);
const CLSID_TF_INPUT_PROCESSOR_PROFILES: GUID =
    GUID::from_u128(0x33c53a50_f456_4884_b049_85fd643e_cfed);
const CHATTERFLY_CLSID: GUID = GUID::from_u128(0x604a99e3_6d90_4571_824d_2639bd572f6c);
const CHATTERFLY_PROFILE: GUID = GUID::from_u128(0xc16c250b_cf1c_4c58_b068_12aedbed610f);
const LANGID_ZH_CN: u16 = 0x0804;

const VK_LWIN: u32 = 0x5b;
const VK_RWIN: u32 = 0x5c;
const TF_MOD_CONTROL: u32 = 0x0002;
const TF_MOD_LCONTROL: u32 = 0x0080;

fn main() -> windows::core::Result<()> {
    unsafe {
        CoInitializeEx(None, COINIT_APARTMENTTHREADED).ok()?;
        let result = probe();
        CoUninitialize();
        result
    }
}

unsafe fn probe() -> windows::core::Result<()> {
    let thread_mgr: ITfThreadMgr =
        CoCreateInstance(&CLSID_TF_THREAD_MGR, None, CLSCTX_INPROC_SERVER)?;
    let client_id = thread_mgr.Activate()?;

    let keys: ITfKeystrokeMgr = thread_mgr.cast()?;
    let document = thread_mgr.CreateDocumentMgr()?;
    let mut context: Option<ITfContext> = None;
    let mut edit_cookie = 0;
    document.CreateContext(client_id, 0, None, &mut context, &mut edit_cookie)?;
    let context = context.expect("TSF CreateContext succeeded without returning a context");
    document.Push(&context)?;
    thread_mgr.SetFocus(&document)?;

    let profiles: ITfInputProcessorProfileMgr = CoCreateInstance(
        &CLSID_TF_INPUT_PROCESSOR_PROFILES,
        None,
        CLSCTX_INPROC_SERVER,
    )?;
    profiles.ActivateProfile(
        TF_PROFILETYPE_INPUTPROCESSOR,
        LANGID_ZH_CN,
        &CHATTERFLY_CLSID,
        &CHATTERFLY_PROFILE,
        HKL::default(),
        TF_IPPMF_FORSESSION,
    )?;
    std::thread::sleep(std::time::Duration::from_millis(250));
    for (name, key) in [
        (
            "lctrl+lwin",
            TF_PRESERVEDKEY {
                uVKey: VK_LWIN,
                uModifiers: TF_MOD_LCONTROL,
            },
        ),
        (
            "ctrl+lwin",
            TF_PRESERVEDKEY {
                uVKey: VK_LWIN,
                uModifiers: TF_MOD_CONTROL,
            },
        ),
        (
            "lctrl+rwin",
            TF_PRESERVEDKEY {
                uVKey: VK_RWIN,
                uModifiers: TF_MOD_LCONTROL,
            },
        ),
        (
            "ctrl+rwin",
            TF_PRESERVEDKEY {
                uVKey: VK_RWIN,
                uModifiers: TF_MOD_CONTROL,
            },
        ),
    ] {
        match keys.GetPreservedKey(&context, &key) {
            Ok(guid) if guid != GUID::zeroed() => println!("{name}: registered guid={guid:?}"),
            Ok(_) => println!("{name}: not_registered"),
            Err(error) => println!("{name}: query_error code={:?}", error.code()),
        }
    }

    let lang_bar: ITfLangBarItemMgr = thread_mgr.cast()?;
    let items = lang_bar.EnumItems()?;
    let mut item_count = 0u32;
    loop {
        let mut batch: [Option<ITfLangBarItem>; 1] = [None];
        let mut fetched = 0u32;
        items.Next(&mut batch, &mut fetched)?;
        if fetched == 0 {
            break;
        }
        let item = batch[0].take().expect("enumerator returned an empty item");
        let mut info = TF_LANGBARITEMINFO::default();
        item.GetInfo(&mut info)?;
        let description_end = info
            .szDescription
            .iter()
            .position(|unit| *unit == 0)
            .unwrap_or(info.szDescription.len());
        let description = String::from_utf16_lossy(&info.szDescription[..description_end]);
        let tooltip = item
            .GetTooltipString()
            .map(|value| value.to_string())
            .unwrap_or_else(|error| format!("<error {:?}>", error.code()));
        let button_text = item
            .cast::<ITfLangBarItemButton>()
            .and_then(|button| unsafe { button.GetText() })
            .map(|value| value.to_string())
            .unwrap_or_default();
        println!(
            "langbar service={:?} item={:?} style=0x{:08x} description={description:?} tooltip={tooltip:?} text={button_text:?}",
            info.clsidService, info.guidItem, info.dwStyle
        );
        item_count += 1;
    }

    thread_mgr.Deactivate()?;
    println!("client_id={client_id} langbar_items={item_count} result=completed");
    Ok(())
}
