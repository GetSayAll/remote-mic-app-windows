# vRemoter Product Context

## Product register

vRemoter is a focused macOS utility for people who use Doubao Input Method
with supported Bluetooth voice remotes. It combines the Mac microphone and
remote microphone, translates the remote voice gesture into the same trigger
configured in Doubao, and provides device-scoped button remapping.

## Core users and jobs

- A Mac user who wants to start and stop Doubao voice input without reaching
  for the keyboard.
- A user with an X6 or Chromecast Voice Remote who wants TV-oriented buttons
  to perform useful Mac keyboard actions.
- A user who may replace a remote with another unit of the same supported
  model and expects the configuration to continue working.

## Product promise

Make supported remotes feel like dependable Mac input devices: setup should
be understandable, voice behavior should be predictable, and mappings should
be reversible.

## Personality

Professional, direct, calm, and trustworthy. The interface should feel like
an instrument panel rather than a playful remote-control skin.

## Interaction principles

- Keep the accepted Studio Mixer as the primary audio surface.
- Put button configuration on an adjacent, dedicated mapping surface.
- Identify supported hardware by model profile, not a single unit's Bluetooth
  UUID. Show exact profile names and connection state.
- Default the Doubao trigger to Option. Always remind users that the selected
  key must match Doubao Input Method's voice-input setting.
- Prefer a visual remote diagram with a clearly paired mapping inspector.
- Offer useful defaults, explicit reset, and a disabled action for every
  remappable button.
- Do not claim support for a model or button until its reports and end behavior
  have been verified on macOS.

## Accessibility and quality bar

- All controls have text labels, keyboard focus, and VoiceOver descriptions.
- State is never communicated by color alone.
- The minimum window size remains usable without clipped controls.
- Reduced-motion settings are respected; no decorative motion is required.

## Anti-references

- Do not copy another app's visual trade dress even when using its information
  architecture as research.
- Do not turn the mapping screen into a dense developer-facing HID inspector.
- Do not hide unsupported or unverified behavior behind optimistic labels.
