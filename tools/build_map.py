"""Assembles assets/levels/mission01.txt from row segments so columns always line up.

Run:  python tools/build_map.py   (then check the printed coordinate report).
You can also edit assets/levels/mission01.txt directly - the game reads it at startup.
"""
import os
import re

AL = ',' * 16          # east alley (x39-54) for rows 1-13
YARD = ',' * 22        # yard (x33-54) rows 14-25
rows = [
    ['W' * 56],
    ['W', '.RRRRRR..', '#', 'p............s', '#', 'T..........R', '#', AL, 'W'],
    ['W', '....l....', '#', '.hhh..l...hhh.', '#', '....l....q.6', '#', ',,,,,,,,,,XXXX,,', 'W'],
    ['W', '...t...5.', '#', '......O.......', '#', '...........7', '#', ',,,,,,,,,,XXXX,,', 'W'],
    ['W', '....l....', '#', '.hhh..l...hhh.', '#', 'c...l.......', '#', ',,,,,,o,,,XXXX,,', 'W'],
    ['W', '.........', 'v', '.............a', '#', '.......%....', '#', AL, 'W'],
    ['W', '####K####', '#', '#GGG##S##GGG##', '#', '#####L######', '#', AL, 'W'],
    ['W', 'c___l_________l_________f________l___', '#', AL, 'W'],
    ['W', '_________a_________g_________________', '#', AL, 'W'],
    ['W', '#####D######', '#', '#####D#####', '#', '............', '#', AL, 'W'],
    ['W', '_p____l____1', '#', '....l......', '#', '.hhhhh......', 'D', ',,,o,,,,,,,,,,,,', 'W'],
    ['W', '____________', '#', '...........', '#', '......l.....', '#', AL, 'W'],
    ['W', '_+__________', '#', '..2........', '#', '...g........', '#', AL, 'W'],
    ['W', '##########v#', '#', '###########', '#', '############', '#', AL, 'W'],
    ['W', '##########V#####################', YARD, 'W'],
    ['W', '.p....', '#', 's', '==', 'v', '===l=======l========', '#', ',,,o,,,,,,,,,,,,,,o,,,', 'W'],
    ['W', '...l..', '#', '====CC====BB====BB=====C', '#', ',,,,,,,BBB,,,,,,,,,,,,', 'W'],
    ['W', '..3...', 'D', '====CC====BB==g=BB======', '#', ',,,,,,,BBB,,,,g,,,,,,,', 'W'],
    ['W', '......', '#', '============a===========', '#', 'c,,,,,,,,,,,,,,,,,,BB,', 'W'],
    ['W', '#######', '==C=====l=======l=====CC', '#', ',,,,,,,,,,,,,,,,,,8BB,', 'W'],
    ['W', '==f=====BBB=====BBB===========*', 'K', YARD, 'W'],
    ['W', '========BBB==g==BBB=====CC=====', '#', ',,,,,BBB,,,,,,,,,,,,,,', 'W'],
    ['W', '===============================', '#', ',,,,,BBB,,,,,,,,o,,,,,', 'W'],
    ['W', '==CC===l=======l=======l====CC=', '#', YARD, 'W'],
    ['W', '==CC=====================q==CC=', '#', ',,,,,,,,,,,,,BB,,,,,,,', 'W'],
    ['W', '=!=======+=====================', '#', ',,,,,,,,,,,,,BB,,,,,,,', 'W'],
    ['W', '#####v############L#############', YARD, 'W'],
    ['W', ',' * 24 + 'c' + ',' * 29, 'W'],
    ['W', ',' * 9 + 'o' + ',' * 19 + 'o' + ',' * 17 + 'o' + ',' * 6, 'W'],
    ['W', ',,,BB,,,,,,,,,,,CC,,,,,,,,,,,,,,,,,BBB,,,,,,,,,,,,,,,,', 'W'],
    ['W', ',,,BB,,,,,g,,,,,CC,,,,,,,,,,,,,,,,,BBB' + ',,,,,' + '#########' + ',,', 'W'],
    ['W', ',' * 43 + '#.l...N.#' + ',,', 'W'],
    ['W', ',' * 20 + 'g' + ',' * 22 + '#.+?....#' + ',,', 'W'],
    ['W', ':::PPPP' + ':' * 36 + '##D######' + '::', 'W'],
    ['W', ':' * 15 + 'o' + ':' * 23 + 'o' + ':' * 14, 'W'],
    ['W', '~' * 54, 'W'],
    ['W', '~' * 54, 'W'],
    ['W' * 56],
]
grid = [''.join(r) for r in rows]
bad = [(i, len(g)) for i, g in enumerate(grid) if len(g) != 56]
assert not bad, f"bad row lengths: {bad}"

META = """
# ---- doors -----------------------------------------------------------
lock 31,6 strength=3 key=seckey
lock 19,26 strength=2
keypad 5,6 code=2187 strength=3
keypad 32,20 code=5532 strength=2
# ---- items (marker character -> item) ---------------------------------
item 1 datacube 0
item 2 key seckey Security_Office_Key
item 3 crossbow
item 5 augcanister
item 6 rifle
item 7 ammo_rifle 30
item 8 datacube 3
# ---- terminals --------------------------------------------------------
terminal 26,1 security login=kade_sec pass=nightwatch hack=2
terminal 2,10 pc mail=5 hack=1
terminal 2,15 pc mail=2 hack=1
terminal 11,1 pc mail=4 hack=2
# ---- patrol routes (first coordinate = guard cell) --------------------
route 20,8 3,8 36,8
route 29,12 35,12 35,9 29,9
route 22,17 22,22 28,22 28,18
route 14,21 14,22 6,22 6,25 14,25
route 47,17 47,23 37,23 37,15
route 11,30 11,27 22,27 22,31 11,31
route 21,32 40,32 40,28 21,28
# ---- guards carrying flashlights --------------------------------------
flashlight 47,17
flashlight 11,30
flashlight 21,32
flashlight 22,17
# ---- turrets -----------------------------------------------------------
turret 4,3 always
# ---- friendly npc ------------------------------------------------------
npc 50,31 informant
"""

NOTES = r"""
0|Datacube: IT Reminder|To: Night shift\nThe security office console password has been reset.\n\n  user: kade_sec\n  pass: nightwatch\n\nChange it at first login. I am not resetting it again.\n  - R. Vance, Facility IT|login=kade_sec:nightwatch
1|Contact: Warehouse code|The contact on the east pier says the warehouse side door keypad (east wall, facing the yard) uses code 5532.|code=5532
2|Email: Server room|FROM: d.morrow@hk-meridian\nTO: warehouse-staff\nSUBJECT: server room\n\nServer room keypad is now 2187. Stop propping the door open, the turret logs every time you do.|code=2187
3|Datacube: Rotation|Lobby has one man after 23:00. The camera on the east wall of the warehouse sweeps the yard. There is a blind spot behind the containers.|
4|LAZARUS Research Log|PROJECT LAZARUS - entry 117\nThe nanite lattice now survives full cardiac arrest in 9 of 12 subjects. Kade wants the core moved off-site before the oversight board audit.\nThe subjects were not volunteers.|
5|Email: Director Halvorsen|FROM: e.halvorsen\nSUBJECT: tonight\n\nThe core leaves at dawn. Double the dock patrols and keep the lights on. If anyone asks, it is a routine inventory.|
"""

out = ("# DEUSMOBILE level file. Edit the grid with any text editor.\n"
       "# The legend is documented at the top of src/level.rs\n[grid]\n"
       + "\n".join(grid) + "\n[meta]" + META + "[notes]" + NOTES)
path = os.path.join(os.path.dirname(os.path.abspath(__file__)), '..', 'assets', 'levels', 'mission01.txt')
with open(path, 'w', newline='\n') as f:
    f.write(out)

for line in META.splitlines():
    for m in re.finditer(r'(\d+),(\d+)', line):
        x, z = int(m.group(1)), int(m.group(2))
        print(f"{line.split()[0]:10s} {x:2d},{z:2d} -> '{grid[z][x]}'")
print("rows:", len(grid), "cols:", len(grid[0]))
