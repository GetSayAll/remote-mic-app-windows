use std::path::PathBuf;

/// 选择要导入的 SayAll 按键映射 JSON 文件。
///
/// 对话框运行在短命 STA 线程；`Ok(None)` 仅表示用户取消，Windows API
/// 初始化或调用失败会保留为错误，供上层写入去标识化日志。
#[cfg(windows)]
pub fn pick_button_mapping_import_path() -> Result<Option<PathBuf>, String> {
    pick_json_path(DialogKind::Open)
}

/// 选择按键映射 JSON 的导出位置。
#[cfg(windows)]
pub fn pick_button_mapping_export_path() -> Result<Option<PathBuf>, String> {
    pick_json_path(DialogKind::Save)
}

#[cfg(not(windows))]
pub fn pick_button_mapping_import_path() -> Result<Option<PathBuf>, String> {
    Err("文件选择器仅在 Windows 上可用".to_owned())
}

#[cfg(not(windows))]
pub fn pick_button_mapping_export_path() -> Result<Option<PathBuf>, String> {
    Err("文件选择器仅在 Windows 上可用".to_owned())
}

#[cfg(windows)]
#[derive(Clone, Copy)]
enum DialogKind {
    Open,
    Save,
}

#[cfg(windows)]
fn pick_json_path(kind: DialogKind) -> Result<Option<PathBuf>, String> {
    std::thread::Builder::new()
        .name("sayall-mapping-file-dialog".to_owned())
        .spawn(move || run_json_dialog(kind))
        .map_err(|_| "启动文件选择器线程失败".to_owned())?
        .join()
        .map_err(|_| "文件选择器线程异常退出".to_owned())?
}

#[cfg(windows)]
fn run_json_dialog(kind: DialogKind) -> Result<Option<PathBuf>, String> {
    use windows::core::{Interface, HRESULT, PCWSTR};
    use windows::Win32::System::Com::{
        CoCreateInstance, CoInitializeEx, CoTaskMemFree, CoUninitialize, CLSCTX_INPROC_SERVER,
        COINIT_APARTMENTTHREADED,
    };
    use windows::Win32::UI::Shell::Common::COMDLG_FILTERSPEC;
    use windows::Win32::UI::Shell::{
        FileOpenDialog, FileSaveDialog, IFileDialog, IFileOpenDialog, IFileSaveDialog,
        FOS_FORCEFILESYSTEM, FOS_OVERWRITEPROMPT, FOS_PATHMUSTEXIST, FOS_STRICTFILETYPES,
        SIGDN_FILESYSPATH,
    };

    if unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED) }.is_err() {
        return Err("初始化文件选择器失败".to_owned());
    }

    let result = (|| -> Result<Option<PathBuf>, String> {
        let dialog: IFileDialog = unsafe {
            match kind {
                DialogKind::Open => {
                    let open: IFileOpenDialog =
                        CoCreateInstance(&FileOpenDialog, None, CLSCTX_INPROC_SERVER)
                            .map_err(|_| "创建导入文件选择器失败".to_owned())?;
                    open.cast()
                        .map_err(|_| "创建导入文件选择器失败".to_owned())?
                }
                DialogKind::Save => {
                    let save: IFileSaveDialog =
                        CoCreateInstance(&FileSaveDialog, None, CLSCTX_INPROC_SERVER)
                            .map_err(|_| "创建导出文件选择器失败".to_owned())?;
                    save.cast()
                        .map_err(|_| "创建导出文件选择器失败".to_owned())?
                }
            }
        };

        let title_text = match kind {
            DialogKind::Open => "导入按键映射配置",
            DialogKind::Save => "导出按键映射配置",
        };
        let title = wide(title_text);
        let filter_name = wide("SayAll 按键映射配置 (*.json)");
        let filter_spec = wide("*.json");
        let filters = [COMDLG_FILTERSPEC {
            pszName: PCWSTR(filter_name.as_ptr()),
            pszSpec: PCWSTR(filter_spec.as_ptr()),
        }];
        unsafe {
            dialog
                .SetTitle(PCWSTR(title.as_ptr()))
                .map_err(|_| "设置文件选择器标题失败".to_owned())?;
            dialog
                .SetFileTypes(&filters)
                .map_err(|_| "设置文件类型过滤器失败".to_owned())?;
            let mut options = dialog
                .GetOptions()
                .map_err(|_| "读取文件选择器选项失败".to_owned())?
                | FOS_FORCEFILESYSTEM
                | FOS_PATHMUSTEXIST
                | FOS_STRICTFILETYPES;
            if matches!(kind, DialogKind::Save) {
                options |= FOS_OVERWRITEPROMPT;
                let default_name = wide("SayAll-按键映射.json");
                dialog
                    .SetFileName(PCWSTR(default_name.as_ptr()))
                    .map_err(|_| "设置导出文件名失败".to_owned())?;
                let extension = wide("json");
                dialog
                    .SetDefaultExtension(PCWSTR(extension.as_ptr()))
                    .map_err(|_| "设置导出文件扩展名失败".to_owned())?;
            }
            dialog
                .SetOptions(options)
                .map_err(|_| "设置文件选择器选项失败".to_owned())?;

            if let Err(error) = dialog.Show(None) {
                // HRESULT_FROM_WIN32(ERROR_CANCELLED)
                if error.code() == HRESULT(0x8007_04C7_u32 as i32) {
                    return Ok(None);
                }
                return Err("显示文件选择器失败".to_owned());
            }
            let item = dialog
                .GetResult()
                .map_err(|_| "读取所选文件失败".to_owned())?;
            let raw_path = item
                .GetDisplayName(SIGDN_FILESYSPATH)
                .map_err(|_| "读取所选文件路径失败".to_owned())?;
            let path = raw_path
                .to_string()
                .map(PathBuf::from)
                .map_err(|_| "所选文件路径无效".to_owned());
            CoTaskMemFree(Some(raw_path.0 as _));
            path.map(Some)
        }
    })();

    unsafe { CoUninitialize() };
    result
}

#[cfg(windows)]
fn wide(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(Some(0)).collect()
}

#[cfg(test)]
mod tests {
    #[test]
    #[cfg(windows)]
    fn open_and_save_dialog_com_classes_are_available() {
        let available = std::thread::Builder::new()
            .name("sayall-mapping-dialog-probe".to_owned())
            .spawn(|| {
                use windows::Win32::System::Com::{
                    CoCreateInstance, CoInitializeEx, CoUninitialize, CLSCTX_INPROC_SERVER,
                    COINIT_APARTMENTTHREADED,
                };
                use windows::Win32::UI::Shell::{
                    FileOpenDialog, FileSaveDialog, IFileOpenDialog, IFileSaveDialog,
                };

                if unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED) }.is_err() {
                    return false;
                }
                let open = unsafe {
                    CoCreateInstance::<_, IFileOpenDialog>(
                        &FileOpenDialog,
                        None,
                        CLSCTX_INPROC_SERVER,
                    )
                }
                .is_ok();
                let save = unsafe {
                    CoCreateInstance::<_, IFileSaveDialog>(
                        &FileSaveDialog,
                        None,
                        CLSCTX_INPROC_SERVER,
                    )
                }
                .is_ok();
                unsafe { CoUninitialize() };
                open && save
            })
            .expect("spawn dialog probe")
            .join()
            .expect("dialog probe panicked");
        assert!(available, "IFileOpenDialog/IFileSaveDialog COM 类应可用");
    }
}
