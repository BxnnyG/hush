---
name: Bug report
about: Something does not work as expected
labels: bug
---

**What happened**

**What you expected**

**Steps to reproduce**

**Environment**
- Distro:
- PipeWire / WirePlumber versions (`pipewire --version`, `wireplumber --version`):
- Sordino version (`sordinoctl status`):
- Microphone (model, USB / built-in / Bluetooth):

**Diagnostics** (please attach, they usually answer the first questions)
- `sordinoctl state`
- `journalctl --user -u sordinod -n 100` or the terminal output of `RUST_LOG=debug sordinod`
- `pw-dump > pw-dump.json` (contains device and app names, check it before posting)
