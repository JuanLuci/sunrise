# Entities, players and commands

Pages for mods that work with players, mobs and other entities, and with the server's commands and services.

For custom status-effect behaviour, see [Ticking a registered mob effect](entities/effect-ticks.md).

`EntityRef::body_yaw(host)` reads a mob's stored body rotation on the server, in degrees without normalization. This is separate from its head rotation. It returns `None` for stale or removed entities, clients, players and other non-mobs, including dragon parts. The ender dragon returns its stored flight/body yaw. Native and wasm mods use the same handle method. In a mob goal, the active mob and disjoint live mobs are read from the goal's view.

<!-- section-pages -->
