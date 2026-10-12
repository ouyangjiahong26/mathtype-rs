#!/usr/bin/env python3
"""Own the X11 CLIPBOARD selection and serve several data formats for one equation.

Usage: clipboard_owner.py --target MIME --file PATH [--target MIME --file PATH ...]

The process stays alive serving the targets until another program takes the
selection; that is how X11 clipboards work, the owner must exist. Equation
payloads are a few kilobytes, far below the single-property size limit, so no
INCR transfers are needed. Requires python-xlib only.

The file also serves as the format contract for the writer: the targets it is
given are the formats the clipboard module advertises.
"""
import argparse
import sys

from Xlib import X, display
from Xlib.protocol import event


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--target", action="append", required=True, metavar="MIME")
    parser.add_argument("--file", action="append", required=True, metavar="PATH")
    parser.add_argument("--display")
    args = parser.parse_args()
    if len(args.target) != len(args.file):
        sys.exit("each --target needs one --file")

    payloads = {}
    for mime, path in zip(args.target, args.file):
        with open(path, "rb") as handle:
            payloads[mime] = handle.read()

    disp = display.Display(args.display)
    screen = disp.screen()
    owner = screen.root.create_window(-1, -1, 1, 1, 0, screen.root_depth)
    clip = disp.intern_atom("CLIPBOARD")
    targets_atom = disp.intern_atom("TARGETS")
    atom_atom = disp.intern_atom("ATOM")
    atom_of = {mime: disp.intern_atom(mime) for mime in payloads}
    # Aliases a requester may ask for instead of the canonical name.
    aliases = {
        disp.intern_atom(name): payloads[mime]
        for name, mime in {
            "text/plain": "text/plain",
            "text/plain;charset=utf-8": "text/plain",
            "UTF8_STRING": "text/plain",
            "STRING": "text/plain",
            "TEXT": "text/plain",
        }.items()
        if mime in payloads
    }
    for mime in payloads:
        aliases.setdefault(atom_of[mime], payloads[mime])

    owner.set_selection_owner(clip, X.CurrentTime)
    if disp.get_selection_owner(clip).id != owner.id:
        sys.exit("could not take the CLIPBOARD selection")

    while True:
        ev = disp.next_event()
        if ev.type == X.SelectionClear:
            return
        if ev.type != X.SelectionRequest or ev.selection != clip:
            continue
        prop = ev.property if ev.property != 0 else ev.target
        if ev.target == targets_atom:
            served = sorted(set(list(aliases.keys()) + [targets_atom]))
            blob = b"".join(atom.to_bytes(4, "little") for atom in served)
            ev.requestor.change_property(prop, atom_atom, 32, blob)
        elif ev.target in aliases:
            ev.requestor.change_property(prop, ev.target, 8, aliases[ev.target])
        else:
            prop = 0
        note = event.SelectionNotify(
            event_type=X.SelectionNotify,
            window=ev.requestor,
            requestor=ev.requestor,
            selection=ev.selection,
            target=ev.target,
            property=prop,
            time=ev.time,
        )
        disp.send_event(ev.requestor, note)
        disp.flush()


if __name__ == "__main__":
    main()
