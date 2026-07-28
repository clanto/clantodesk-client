Name:       clantodesk
Version:    1.4.9
Release:    0
Summary:    RPM package
License:    GPL-3.0
URL:        https://rustdesk.com
Vendor:     rustdesk <info@rustdesk.com>
Requires:   gtk3 libxcb libXfixes alsa-lib libva pam gstreamer1-plugins-base
Recommends: libayatana-appindicator-gtk3 libxdo
Provides:   libdesktop_drop_plugin.so()(64bit), libdesktop_multi_window_plugin.so()(64bit), libfile_selector_linux_plugin.so()(64bit), libflutter_custom_cursor_plugin.so()(64bit), libflutter_linux_gtk.so()(64bit), libscreen_retriever_plugin.so()(64bit), libtray_manager_plugin.so()(64bit), liburl_launcher_linux_plugin.so()(64bit), libwindow_manager_plugin.so()(64bit), libwindow_size_plugin.so()(64bit), libtexture_rgba_renderer_plugin.so()(64bit)

# https://docs.fedoraproject.org/en-US/packaging-guidelines/Scriptlets/

%description
The best open-source remote desktop client software, written in Rust.

%prep
# we have no source, so nothing here

%build
# we have no source, so nothing here

# %global __python %{__python3}

%install

mkdir -p "%{buildroot}/usr/share/clantodesk" && cp -r ${HBB}/flutter/build/linux/x64/release/bundle/* -t "%{buildroot}/usr/share/clantodesk"
mkdir -p "%{buildroot}/usr/bin"
install -Dm 644 $HBB/res/clantodesk.service -t "%{buildroot}/usr/share/clantodesk/files"
install -Dm 644 $HBB/res/clantodesk.desktop -t "%{buildroot}/usr/share/clantodesk/files"
install -Dm 644 $HBB/res/clantodesk-link.desktop -t "%{buildroot}/usr/share/clantodesk/files"
install -Dm 644 $HBB/res/128x128@2x.png "%{buildroot}/usr/share/icons/hicolor/256x256/apps/clantodesk.png"
install -Dm 644 $HBB/res/scalable.svg "%{buildroot}/usr/share/icons/hicolor/scalable/apps/clantodesk.svg"

%files
/usr/share/clantodesk/*
/usr/share/clantodesk/files/clantodesk.service
/usr/share/icons/hicolor/256x256/apps/clantodesk.png
/usr/share/icons/hicolor/scalable/apps/clantodesk.svg
/usr/share/clantodesk/files/clantodesk.desktop
/usr/share/clantodesk/files/clantodesk-link.desktop

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
ln -sf /usr/share/clantodesk/clantodesk /usr/bin/clantodesk
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
    rm /usr/bin/clantodesk || true
    rmdir /usr/lib/clantodesk || true
    rmdir /usr/local/clantodesk || true
    rmdir /usr/share/clantodesk || true
    rm /usr/share/applications/clantodesk.desktop || true
    rm /usr/share/applications/clantodesk-link.desktop || true
    update-desktop-database
  ;;
  1)
    # for upgrade
    rmdir /usr/lib/clantodesk || true
    rmdir /usr/local/clantodesk || true
  ;;
esac
