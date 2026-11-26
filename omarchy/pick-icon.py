#!/usr/bin/env python3
"""Open the native XDG Desktop Portal file chooser to pick a space icon image."""

import json
import os
import secrets
import sys
import urllib.parse

import gi

gi.require_version("Gio", "2.0")
gi.require_version("GLib", "2.0")
from gi.repository import Gio, GLib  # noqa: E402


def start_folder(space_id: str | None) -> str:
    if space_id:
        state_home = os.environ.get("XDG_STATE_HOME") or os.path.join(
            os.path.expanduser("~"), ".local", "state"
        )
        spaces_json = os.path.join(state_home, "omarchy-spaces", "spaces.json")
        try:
            with open(spaces_json, encoding="utf-8") as f:
                data = json.load(f)
            entry = (data.get("spaces") or {}).get(space_id) or {}
            folder = entry.get("folder")
            if isinstance(folder, str) and os.path.isdir(folder):
                return folder
        except (OSError, ValueError):
            pass
    return os.path.expanduser("~")


def main() -> int:
    space_id = sys.argv[1] if len(sys.argv) > 1 and sys.argv[1] else None
    folder = start_folder(space_id)

    bus = Gio.bus_get_sync(Gio.BusType.SESSION, None)
    sender = bus.get_unique_name().lstrip(":").replace(".", "_")
    token = f"omarchy_spaces_icon_{os.getpid()}_{secrets.token_hex(4)}"
    request_path = f"/org/freedesktop/portal/desktop/request/{sender}/{token}"

    loop = GLib.MainLoop()
    chosen: list[str] = []

    def on_response(_conn, _sender, _path, _iface, _signal, params, _data):
        code, results = params.unpack()
        if code == 0:
            uris = results.get("uris") or []
            if uris and isinstance(uris[0], str) and uris[0].startswith("file://"):
                chosen.append(urllib.parse.unquote(urllib.parse.urlparse(uris[0]).path))
        loop.quit()

    sub_id = bus.signal_subscribe(
        "org.freedesktop.portal.Desktop",
        "org.freedesktop.portal.Request",
        "Response",
        request_path,
        None,
        Gio.DBusSignalFlags.NO_MATCH_RULE,
        on_response,
        None,
    )

    image_filter = GLib.Variant(
        "(sa(us))",
        (
            "Images (PNG, SVG, JPG, WebP, ICO)",
            [
                (0, "*.png"),
                (0, "*.svg"),
                (0, "*.jpg"),
                (0, "*.jpeg"),
                (0, "*.webp"),
                (0, "*.ico"),
            ],
        ),
    )
    options = {
        "handle_token": GLib.Variant("s", token),
        "accept_label": GLib.Variant("s", "Choose"),
        "multiple": GLib.Variant("b", False),
        "current_folder": GLib.Variant("ay", os.fsencode(folder) + b"\x00"),
        "filters": GLib.Variant("a(sa(us))", [image_filter.unpack()]),
    }

    try:
        bus.call_sync(
            "org.freedesktop.portal.Desktop",
            "/org/freedesktop/portal/desktop",
            "org.freedesktop.portal.FileChooser",
            "OpenFile",
            GLib.Variant("(ssa{sv})", ("", "Choose space icon", options)),
            GLib.VariantType("(o)"),
            Gio.DBusCallFlags.NONE,
            -1,
            None,
        )
        loop.run()
    finally:
        bus.signal_unsubscribe(sub_id)

    if chosen:
        print(chosen[0])
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
