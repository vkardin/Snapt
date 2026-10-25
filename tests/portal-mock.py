#!/usr/bin/env python3
"""A fake xdg-desktop-portal for the ignored tests in src/portal.rs.

It answers Screenshot with a 64x32 PNG, Background by allowing what it is
asked for, and GlobalShortcuts by binding what it is asked for and pressing
the shortcut twice. Run the tests with:

    dbus-run-session -- sh -c 'python3 tests/portal-mock.py & sleep 1; cargo test -- --ignored portal'

Needs python3-dbus and python3-gi.
"""
import os, struct, tempfile, zlib

import dbus, dbus.service, dbus.mainloop.glib
from gi.repository import GLib
dbus.mainloop.glib.DBusGMainLoop(set_as_default=True)
bus = dbus.SessionBus()
name = dbus.service.BusName("org.freedesktop.portal.Desktop", bus)
P = "/org/freedesktop/portal/desktop"
SHOT = os.path.join(tempfile.gettempdir(), "snapt portal test.png")

def png(path, w, h):
    rows = b"".join(b"\0" + b"".join(bytes([x*255//w, 0, 0, 255]) for x in range(w)) for y in range(h))
    def chunk(t, d): return struct.pack(">I", len(d)) + t + d + struct.pack(">I", zlib.crc32(t + d) & 0xffffffff)
    open(path, "wb").write(b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", struct.pack(">IIBBBBB", w, h, 8, 6, 0, 0, 0)) + chunk(b"IDAT", zlib.compress(rows)) + chunk(b"IEND", b""))

class Req(dbus.service.Object):
    @dbus.service.signal("org.freedesktop.portal.Request", signature="ua{sv}")
    def Response(self, code, results): pass

class Sess(dbus.service.Object):
    @dbus.service.method("org.freedesktop.portal.Session", in_signature="", out_signature="")
    def Close(self):
        print("session closed", flush=True)

def respond(sender, token, results):
    path = f"{P}/request/{sender[1:].replace('.', '_')}/{token}"
    r = Req(bus, path)
    GLib.timeout_add(50, lambda: (r.Response(dbus.UInt32(0), dbus.Dictionary(results, signature="sv")), False)[1])
    return dbus.ObjectPath(path)

class Portal(dbus.service.Object):
    @dbus.service.method("org.freedesktop.portal.Screenshot", in_signature="sa{sv}", out_signature="o", sender_keyword="sender")
    def Screenshot(self, parent, opts, sender):
        print("Screenshot", dict(opts), flush=True)
        png(SHOT, 64, 32)
        return respond(sender, opts["handle_token"], {"uri": "file://" + SHOT.replace(" ", "%20")})

    @dbus.service.method("org.freedesktop.portal.Background", in_signature="sa{sv}", out_signature="o", sender_keyword="sender")
    def RequestBackground(self, parent, opts, sender):
        print("RequestBackground", dict(opts), flush=True)
        return respond(sender, opts["handle_token"], {"background": True, "autostart": bool(opts["autostart"])})

    @dbus.service.method("org.freedesktop.portal.GlobalShortcuts", in_signature="a{sv}", out_signature="o", sender_keyword="sender")
    def CreateSession(self, opts, sender):
        print("CreateSession", dict(opts), flush=True)
        spath = f"{P}/session/{sender[1:].replace('.', '_')}/{opts['session_handle_token']}"
        Sess(bus, spath)
        self.session = spath
        return respond(sender, opts["handle_token"], {"session_handle": spath})

    @dbus.service.method("org.freedesktop.portal.GlobalShortcuts", in_signature="oa(sa{sv})sa{sv}", out_signature="o", sender_keyword="sender")
    def BindShortcuts(self, session, shortcuts, parent, opts, sender):
        print("BindShortcuts", session, [(s, dict(o)) for s, o in shortcuts], flush=True)
        out = dbus.Array([dbus.Struct((s, dbus.Dictionary({"description": o["description"], "trigger_description": o["preferred_trigger"]}, signature="sv"))) for s, o in shortcuts], signature="(sa{sv})")
        def fire():
            self.Activated(dbus.ObjectPath(session), "capture", dbus.UInt64(1), dbus.Dictionary({}, signature="sv"))
            return False
        GLib.timeout_add(300, fire)
        GLib.timeout_add(400, fire)
        return respond(sender, opts["handle_token"], {"shortcuts": out})

    @dbus.service.signal("org.freedesktop.portal.GlobalShortcuts", signature="osta{sv}")
    def Activated(self, session, id, ts, opts): pass

    @dbus.service.method("org.freedesktop.DBus.Properties", in_signature="ss", out_signature="v")
    def Get(self, iface, prop):
        return dbus.UInt32(1)

    @dbus.service.method("org.freedesktop.DBus.Properties", in_signature="s", out_signature="a{sv}")
    def GetAll(self, iface):
        return dbus.Dictionary({"version": dbus.UInt32(1)}, signature="sv")

Portal(bus, P)
print("ready", flush=True)
GLib.MainLoop().run()
