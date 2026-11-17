# Elo privacy policy

*Last updated: November 17, 2026* · [Português](PRIVACIDADE.md)

Elo is an open-source remote access app published by Stringari Consultoria.
This policy covers the app in every store and download.

## What Stringari Consultoria collects

**Nothing.** Elo has no Stringari account, sends no telemetry, usage
statistics or crash reports, and does not go through any server of ours. We
do not sell or share data, because we never receive it.

## Where data goes

To connect two computers, Elo uses a **connection server chosen by whoever
uses the app**: the server built into Elo itself, or your company's server
(`elo-server`). That server belongs to whoever installs it, not to Stringari.

- **Screen, audio, keyboard, mouse, clipboard, chat and files** in a session
  are end-to-end encrypted (SPAKE2 + ChaCha20-Poly1305) between the two
  computers. The connection server only relays the encrypted data and cannot
  read it.
- **The connection server stores**, on its own machine:
  - the ID, name, operating system, MAC address and IP address of each
    computer that connects to it, and when it was last seen;
  - the session history: who accessed whom, start, end and amount of data
    (kept for 90 days by default; whoever runs the server can change this);
  - technician accounts and companies, if that feature is used.

Whoever runs the connection server (you or your company) is responsible for
that data.

## What stays on your computer

Elo's settings (computer name, server, preferences) are kept in a folder of
your user account and never leave it. The unattended access password is not
stored: Elo keeps only a hash of it (Argon2id, salted), and neither the
password nor the hash is sent to anyone, not even the connection server. They
only prove identity inside the encryption (SPAKE2). Deleting that folder
removes everything (the Microsoft Store version deletes it on uninstall).

## Other connections

- **Updates:** versions downloaded from GitHub check
  `github.com/Stringari-Consultoria/elo` for a new version. The Microsoft
  Store version does not; the Store updates it.
- **Public address test:** when you ask, Elo tries to connect to the public
  address you configured yourself.

## Permissions

Elo captures the screen and audio and controls the mouse and keyboard **only
during a session** that you accepted on screen or allowed with the unattended
access password. During a session, Elo shows who is connected, and you can
end the session at any time.

## Children

Elo is not directed at children and collects no one's data.

## Contact

Questions: open an issue at
<https://github.com/Stringari-Consultoria/elo/issues>.
