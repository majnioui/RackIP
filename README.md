# RackIP

Switch the IP address of your Ethernet port in one click. It's built for data
center work, where you plug a laptop straight into a server's management or
service port and need an address on its subnet.

The normal way on macOS is System Settings › Network › *adapter* › Details ›
TCP/IP › Manually, then typing the IP, mask and router. RackIP does the same
thing from one small window or from the menu bar.

## Features

- **Port picker**: lists your wired adapters (built-in, USB, Thunderbolt, docks)
  with a live *connected* indicator, so you can tell which dongle has the cable.
- **Apply a static IP**: IP address is required. Subnet mask and gateway are
  optional (the mask defaults to `255.255.255.0`). You can also type CIDR
  (`10.0.0.5/24`), and the mask field takes a prefix (`22` or `/22`).
- **Saved addresses**: every address you apply is saved. Next time you're at
  that server, pick it from the dropdown (or the menu bar) and you're done.
  Name them if you like ("iDRAC rack 12").
- **Back to DHCP**: one button.
- **Input checks**: rejects bad addresses, non-contiguous masks, and
  network/broadcast addresses. Warns when the gateway is outside the subnet.
- **Menu bar**: apply any saved address without opening the window.

> **Tip:** leave the gateway empty while you work on an isolated management
> network. Your internet traffic then stays on Wi-Fi, and only the server's
> subnet goes through the cable.

## Install

Download `RackIP-x.y.z.dmg` from
[Releases](https://github.com/majnioui/RackIP/releases/latest), open it and
drag RackIP to Applications. Requires macOS 13 or later, on Apple Silicon or
Intel.

### Build from source

Requires the Xcode Command Line Tools (`xcode-select --install`).

```sh
git clone https://github.com/majnioui/RackIP.git
cd RackIP
scripts/bundle.sh          # builds build/RackIP.app
cp -R build/RackIP.app /Applications/
```

## How it works

RackIP runs `/usr/sbin/networksetup`, the same backend System Settings uses,
so every change also shows up in System Settings:

| Action       | Command |
|--------------|---------|
| List ports   | `networksetup -listnetworkserviceorder` |
| Read current | `networksetup -getinfo "<port>"` |
| Static IP    | `networksetup -setmanual "<port>" <ip> <mask> [<gateway>]` |
| DHCP         | `networksetup -setdhcp "<port>"` |

On an administrator account these run without a prompt. On a standard account,
RackIP asks for an admin password through the normal macOS dialog.

Saved addresses are stored in the app's preferences
(`defaults read com.majnioui.rackip`).

## Development

```sh
swift build                 # debug build
swift run core-checks       # address validation + networksetup parsing checks
scripts/bundle.sh           # release .app (universal, ad-hoc signed)
scripts/dmg.sh              # build/RackIP-<version>.dmg
```

- `Sources/RackIPCore`: validation and output parsing, with no UI code.
- `Sources/RackIP`: the SwiftUI app (window and menu bar).
- `Sources/CoreChecks`: checks that run without Xcode.

## Roadmap

- Windows version (`netsh interface ipv4 set address`)
- DNS servers per saved address
- Ping / reachability check after applying
- Import/export saved addresses to share with a team

## License

MIT
