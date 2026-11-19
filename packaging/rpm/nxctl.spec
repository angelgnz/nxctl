%{!?_udevrulesdir:%define _udevrulesdir /usr/lib/udev/rules.d}
%{!?_unitdir:%define _unitdir /usr/lib/systemd/system}
%{!?_userunitdir:%define _userunitdir /usr/lib/systemd/user}
%{!?_systemd_util_dir:%define _systemd_util_dir /usr/lib/systemd}

Name:           nxctl
Version:        0.1.4
Release:        1%{?dist}
Summary:        Open-source Linux alternative to NZXT CAM over liquidctl (CLI, TUI, daemon, and GUI)

License:        MIT OR Apache-2.0
URL:            https://github.com/angelgnz/nxctl
Source0:        https://github.com/angelgnz/nxctl/archive/refs/tags/v%{version}.tar.gz

BuildRequires:  cargo
BuildRequires:  rust
BuildRequires:  pkgconfig(xkbcommon)
BuildRequires:  pkgconfig(wayland-client)
BuildRequires:  pkgconfig(x11)
BuildRequires:  pkgconfig(xcb)
BuildRequires:  pkgconfig(gl)

Recommends:     liquidctl

%description
nxctl is an open-source Linux fan/pump curve controller, RGB lighting manager,
pywal synchronizer, TUI, systemd daemon, and desktop GUI built over liquidctl,
Linux hwmon, and nvidia-smi.

%prep
%autosetup -n nxctl-%{version}

%build
cargo build --release --workspace --locked

%install
install -Dm755 target/release/nxctl %{buildroot}%{_bindir}/nxctl
install -Dm755 target/release/nxctld %{buildroot}%{_bindir}/nxctld
install -Dm755 target/release/nxctl-gui %{buildroot}%{_bindir}/nxctl-gui

install -Dm644 contrib/72-nzxt-hwmon.rules %{buildroot}%{_udevrulesdir}/72-nzxt-hwmon.rules
install -Dm644 contrib/nxctld.service %{buildroot}%{_unitdir}/nxctld.service
install -Dm644 contrib/nxctld-user.service %{buildroot}%{_userunitdir}/nxctld.service
install -Dm755 contrib/nxctl-sleep %{buildroot}%{_systemd_util_dir}/system-sleep/nxctl-sleep

install -Dm644 contrib/nxctl-gui.desktop %{buildroot}%{_datadir}/applications/nxctl-gui.desktop
install -Dm644 contrib/nxctl.svg %{buildroot}%{_datadir}/icons/hicolor/scalable/apps/nxctl.svg
install -Dm644 contrib/nxctl-tray.svg %{buildroot}%{_datadir}/icons/hicolor/scalable/status/nxctl-tray.svg

%post
if [ -x /usr/bin/udevadm ]; then
    /usr/bin/udevadm control --reload-rules >/dev/null 2>&1 || :
    /usr/bin/udevadm trigger -s hwmon >/dev/null 2>&1 || :
fi

%files
%doc README.md
%{_bindir}/nxctl
%{_bindir}/nxctld
%{_bindir}/nxctl-gui
%{_udevrulesdir}/72-nzxt-hwmon.rules
%{_unitdir}/nxctld.service
%{_userunitdir}/nxctld.service
%{_systemd_util_dir}/system-sleep/nxctl-sleep
%{_datadir}/applications/nxctl-gui.desktop
%{_datadir}/icons/hicolor/scalable/apps/nxctl.svg
%{_datadir}/icons/hicolor/scalable/status/nxctl-tray.svg
