#!/bin/sh
# AutoScene is gone: the system forgets its .asp type (plugin packages)
# and its menu entry. Either tool may be missing on a minimal system; the
# removal still works.
update-mime-database /usr/share/mime >/dev/null 2>&1 || true
update-desktop-database -q /usr/share/applications >/dev/null 2>&1 || true
