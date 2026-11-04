#!/bin/sh
# AutoScene opens .asp files (plugin packages): the system learns the type
# and the menu entry that takes it. Either tool may be missing on a minimal
# system; the install still works.
update-mime-database /usr/share/mime >/dev/null 2>&1 || true
update-desktop-database -q /usr/share/applications >/dev/null 2>&1 || true
