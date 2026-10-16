# Zeron user guide

Zeron is one desktop app with two areas:

- **Library** is where you learn: you import courses, watch and read lessons, and take notes.
- **Publish** is where you make courses and share them.

Courses travel as BitTorrent packages, so they work offline and need no server.

## Library

### Getting a course

- **From a folder:** "Import local course" verifies every file against the course's `course.json` before the course appears.
- **From a magnet link or `.torrent` file:** use the torrent panel. Zeron downloads only `course.json` and the publisher's signature first, shows the course outline, and downloads lessons as you open them.
- **From a catalog:** Library → Catalogs → "Add catalog file…" or "Follow address…". A catalog is a signed list of a publisher's courses. Choose Import on any course, or Update when a newer version is listed.
- **From YouTube:** paste a video link under "Learn from a YouTube video" and choose Add video.
  - The video plays in YouTube's own player and needs an internet connection. Zeron never downloads it.
  - Notes at a moment, sketches, progress, and search work as for any course, and stay on this computer.
  - Playlists are not supported yet.
  - Some owners do not allow their videos to be played outside YouTube. Zeron tells you when that is the case.

### Who published a course

The course page says one of:

- **Verified publisher**, with a fingerprint such as `7F3A-91C2-0B44-E8D1`. The first signed version you import pins the course to its publisher. After that, Zeron refuses a newer version signed by anyone else, or one with no signature, and keeps your copy. Compare the fingerprint with the one your publisher gives you, for example by message or phone.
- **Unsigned course.** Zeron cannot tell who made it, and anyone could publish an update of it.

If your publisher tells you they really have a new identity, use "Forget publisher…". The next signed version you import will be trusted, and your notes and progress stay.

### Learning

- **Media:** videos and audio play while they download. Only bytes verified against the torrent are ever played.
- **Notes** can point at a moment in a video, a page or region of a PDF, or the whole lesson.
- **Drawings:** each lesson has a drawing canvas.
- **Progress:** a lesson is marked as watched at 90%, or by hand.
- **Search** covers course and lesson titles, your notes, transcripts, and lesson text, offline.
- **After an update:** if a new version of a course renames or replaces lessons, the course page lists the notes the new version no longer shows and suggests where they belong. Nothing moves until you choose "Move here".

### Keeping lessons offline

Mark a lesson, module, or course to keep. Lessons you only streamed are removed first when the cache is full. Pause a course to stop its transfers.

## Publish

### Making a course

1. Choose "New course" and fill in the details.
2. Add modules and lessons. You can drop folders and files onto the Builder, or use "Add folders…" and "Add files…".
3. **Prepare release** copies the files into a package, writes `course.json`, and builds and verifies the torrent. Your source files are only read.
4. **Publish release** makes the version permanent. A version number is never reused.

**Fixing a published course:** "New draft from this version" starts the next version with the same lessons, so learners' notes stay attached. "Make current…" rolls back to an older version.

### Signing

**Publish → Identity → "Create signing identity"** creates a key in your computer's keychain:

- the macOS Keychain on a Mac;
- the Windows Credential Manager on Windows;
- GNOME Keyring or KWallet on Linux.

From then on every release is signed.

- **Save a backup of the key** with a long passphrase. Without the keychain entry or a backup, nobody can sign updates of your courses again.
- **Rotate the key** if you think a computer was compromised. Learners keep trusting you, because your old key signs the handover. Then, in Publish → Catalog, revoke the retired key.

### Sharing

- **Distribution:** start seeding a release. Learners need you, or another learner who shares the course, to be online.
- **Catalog:** confirm each release you want listed, then "Export signed catalog…". Put the file anywhere learners can reach, ideally an `https://` address they can follow.
- **Network** (Settings) chooses who can reach you: Local network or Internet. It also sets the port, router forwarding, and speed limits.

## Settings

- **Backup** saves your library, notes, progress, catalogs, and Publish drafts and release records. Restoring takes effect when Zeron restarts, and the previous data is kept in a `before-restore-…` folder. A backup does not hold course videos, release files, or your signing key.
- **Updates:** Zeron checks for a new version only when you click "Check for updates", and installs only updates signed with Zeron's release key.
- **Publisher configuration** (Publish → Distribution) moves trackers and Network settings to another computer.
