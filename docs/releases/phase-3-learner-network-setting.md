# Phase 3: learner network setting

Status: complete. Learners choose where Zeron looks for torrent courses and who can reach the courses they share. Like the Admin, the choice applies the next time Zeron starts.

## Delivered

- **Learner backend:**
  - `get_network_settings` returns the stored profile, the profile the engines are running with, whether the torrent engine is available, and this session's two peer ports: downloads (the private engine) and sharing.
  - `set_network_profile` stores `loopback`, `local_network`, or `internet` through `CourseLibrary::set_network_profile` and rejects anything else as `invalid_input`.
  - Both engines start with the stored profile, which defaults to Local network.
- **`learning-application`** re-exports `TorrentEngine` so the composition root can read peer ports without depending on the port crate.
- **Learner UI:**
  - A "Network" header button opens a Network view with Back.
  - The view offers three choices, worded from the learner's side:
    - **Local network:** finds courses shared on the local network and announces nothing to the internet.
    - **Internet:** uses the public DHT. The IP address becomes visible to other DHT users, and shared courses become reachable from the internet.
    - **This computer only:** for testing.
  - A changed choice shows "Restart Zeron to apply the new network setting". Otherwise the view says which ports downloads and shared courses use, and that UPnP is never used.
  - An unavailable engine and failed saves are explained.
  - The header wraps on narrow windows.

## Acceptance evidence

- **Library test:** the profile defaults to Local network and persists across reopen. Profile names round-trip in `torrent-port`.
- **Vitest (5 new tests):**
  - the active profile with both ports and the UPnP note;
  - choosing Internet saves it and shows the restart notice with the IP warning;
  - an unavailable engine and a failed save are explained;
  - Back returns to the previous view;
  - the header button opens the view and returns to the library.
- **Real window** (WebDriver, existing Learner data):
  - **Saving Internet:** the Network view showed the restart notice with Internet selected.
  - **After restarting under Internet:** the view reported download port 44937 and sharing port 34065, matching the process's TCP listeners. The process had two more UDP sockets than under Local network, one DHT socket per engine.
  - **Switching back to Local network and restarting:** the DHT sockets were gone and the notice cleared.
  - At the window's minimum width (760 px), neither view scrolls horizontally.
- All repository gates pass.

## Current limitations

- A profile change needs a restart; the engines are not restarted live.
- The Internet profile was checked only for its sockets. Finding a course over the public DHT was not tried, because tests must not depend on public networks.
- Ports are ephemeral and change with each launch; there is no fixed-port option yet.
