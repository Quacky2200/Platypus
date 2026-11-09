# Rewind

A physics world you can run backwards: knock a wall of crates down with a cannon, then hold **R** and
time flows back: every crate and ball returns, exactly as it fell. Let go to play on from there.

| Key | |
|---|---|
| A / D | turn the camera round the wall |
| W / S | camera in and out |
| Space or left mouse | fire the next cannonball |
| hold R | rewind (Shift: twice as fast) |

## How it works

The engine can copy the whole physics world and put it back (`Physics.snapshot()`,
`Physics.restore(snapshot)`, `Physics.dropSnapshot(snapshot)`, `docs/platypus-api.md` "Rewindable
physics"). The game keeps a snapshot every other physics step for the last eight seconds, a buffer
of about 240. Rewinding takes the newest off the buffer and restores the one before it, once a step;
the objects on the bodies follow. Letting go simply goes on stepping from the restored world, and
because a restored world steps exactly as it did the first time, the same shot knocks the wall down
the same way again.

Nothing is created or destroyed while playing: the cannonballs are ten balls that wait in a row, and a
shot moves the next one to the cannon. Everything the world holds is then part of a snapshot. (A body
made after a snapshot is gone after a restore, one removed since is back: the objects of a game that
makes and destroys things have to be kept in step by the script.)

`-- --demo` fires two shots by itself and rewinds for a while, and `--debug` prints the wall's state
twice a second: `platypus-app examples/rewind --headless --tick-rate 600 --frames 6000 -- --demo --debug`
shows 42 crates standing, 27 after the shots, and 42 again when the rewind is over.

Test: `M8-PHY-01..04` (`TESTS.md`).
