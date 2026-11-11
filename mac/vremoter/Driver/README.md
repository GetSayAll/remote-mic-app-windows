# vRemote Driver HAL builder

This is the reproducible driver builder used by vRemoter.
Doubao filters CoreAudio devices whose transport type is `virtual`, so the
generated device uses a USB transport identity while remaining a user-space
HAL plug-in.

The build script:

1. copies the installed `BlackHole2ch.driver` into this folder;
2. gives the copy a distinct bundle ID, factory UUID, device UID, model UID,
   box name, and display name;
3. leaves the driver's outer Box transport as `virt`, but changes the actual
   audio Device transport to `usb ` in both Intel and Apple Silicon slices;
4. applies an ad-hoc local signature.

It does not modify the installed BlackHole driver. The generated bundle is:

`build/vRemoteDriver.driver`

This remains a binary-patching build based on the locally installed universal
BlackHole 2ch driver. A future release should replace it with a source build
that has the same identifiers and transport setting.

`install-driver.sh` installs only the new side-by-side bundle and refuses to
overwrite an existing copy. `uninstall-driver.sh` removes only that bundle.
Both require administrator privileges.
