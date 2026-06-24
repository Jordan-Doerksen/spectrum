# Spectrum — setup (Riley)

Spectrum watches the news and posts the important stuff to our Discord channels
(money · politics · catastrophe). It runs quietly in your system tray.

## Two double-clicks, once

1. **SETUP.bat** — installs the local AI (Ollama) and its model. Two big downloads
   (~1.3 GB + ~4.7 GB), one time. Leave it running; if a small installer window
   appears, click through it.
2. **INSTALL-AUTOSTART.bat** — makes Spectrum start with Windows, live in the tray,
   and it starts it right now too.

That's it. It's running, and it'll start on its own every time the PC boots.

## Using it

- It lives in the **system tray** (near the clock). **Click the icon** to open the window.
- The window has **Start / Stop**, a **Dry run** toggle (classify without posting),
  live counts per channel, and an activity log.
- **Right-click the tray icon** → Start / Stop / Quit.
- It's already pointed at our Discord channels — nothing to configure.

## If something looks off

- **No cards posting?** Make sure the **Ollama llama icon** is showing near the clock
  (the AI has to be running). If the model didn't finish, run **SETUP.bat** again.
- Run it by hand: **START.bat**.  Stop it: **STOP.bat**.
- Stop it auto-starting: **UNINSTALL-AUTOSTART.bat**.

> Only this machine should run Spectrum — if two copies run, they'd both post the same
> news. You're the host.
