Name:       clantodesk
Version:    1.4.9
Release:    0
Summary:    RPM package
License:    AGPL-3.0-only
URL:        https://rustdesk.com
Vendor:     rustdesk <info@rustdesk.com>
Requires:   gtk3 libxcb libXfixes alsa-lib libva2 pam gstreamer1-plugins-base
Recommends: libayatana-appindicator-gtk3 libxdo

# https://docs.fedoraproject.org/en-US/packaging-guidelines/Scriptlets/

%description
The best open-source remote desktop client software, written in Rust.

%prep
# we have no source, so nothing here

%build
# we have no source, so nothing here

%global __python %{__python3}

%install
mkdir -p %{buildroot}/usr/bin/
mkdir -p %{buildroot}/usr/share/clantodesk/
mkdir -p %{buildroot}/usr/share/clantodesk/files/
mkdir -p %{buildroot}/usr/share/icons/hicolor/256x256/apps/
mkdir -p %{buildroot}/usr/share/icons/hicolor/scalable/apps/
install -m 755 $HBB/target/release/clantodesk %{buildroot}/usr/bin/clantodesk
install $HBB/libsciter-gtk.so %{buildroot}/usr/share/clantodesk/libsciter-gtk.so
install $HBB/res/clantodesk.service %{buildroot}/usr/share/clantodesk/files/
install $HBB/res/128x128@2x.png %{buildroot}/usr/share/icons/hicolor/256x256/apps/clantodesk.png
install $HBB/res/scalable.svg %{buildroot}/usr/share/icons/hicolor/scalable/apps/clantodesk.svg
install $HBB/res/clantodesk.desktop %{buildroot}/usr/share/clantodesk/files/
install $HBB/res/clantodesk-link.desktop %{buildroot}/usr/share/clantodesk/files/

%files
/usr/bin/clantodesk
/usr/share/clantodesk/libsciter-gtk.so
/usr/share/clantodesk/files/clantodesk.service
/usr/share/icons/hicolor/256x256/apps/clantodesk.png
/usr/share/icons/hicolor/scalable/apps/clantodesk.svg
/usr/share/clantodesk/files/clantodesk.desktop
/usr/share/clantodesk/files/clantodesk-link.desktop
/usr/share/clantodesk/files/__pycache__/*

%changelog
# let's skip this for now

%pre
# can do something for centos7
case "$1" in
  1)
    # for install
  ;;
  2)
    # for upgrade
    systemctl stop clantodesk || true
  ;;
esac

%post
cp /usr/share/clantodesk/files/clantodesk.service /etc/systemd/system/clantodesk.service
cp /usr/share/clantodesk/files/clantodesk.desktop /usr/share/applications/
cp /usr/share/clantodesk/files/clantodesk-link.desktop /usr/share/applications/
systemctl daemon-reload
systemctl enable clantodesk
systemctl start clantodesk
update-desktop-database

%preun
case "$1" in
  0)
    # for uninstall
    systemctl stop clantodesk || true
    systemctl disable clantodesk || true
    rm /etc/systemd/system/clantodesk.service || true
  ;;
  1)
    # for upgrade
  ;;
esac

%postun
case "$1" in
  0)
    # for uninstall
    rm /usr/share/applications/clantodesk.desktop || true
    rm /usr/share/applications/clantodesk-link.desktop || true
    update-desktop-database
  ;;
  1)
    # for upgrade
  ;;
esac
