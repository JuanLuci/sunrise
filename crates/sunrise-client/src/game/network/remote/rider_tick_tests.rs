//! A client rider's own move runs inside `tick_local_player`: its look is this tick's and its
//! ground and collision flags are what the move left, after the seat is taken.
use super::*;
use crate::game::boats::Boat;
use crate::game::native_entity_refs::tests::with_client_game;
use sunrise_client::motion::PlayerMotion;
use winit::keyboard::KeyCode;

#[test]
#[ignore = "daily"]
fn a_riders_move_takes_the_look_of_the_tick_and_keeps_its_ground() {
    mod_platform::testing::with_mods(Vec::new().into(), || {
        with_client_game(|game| {
            let (client, server) = sunrise_protocol::connection::Pipe::pair();
            game.connect_local(client, "rider-tick").unwrap();
            let remote = game.remote.as_mut().unwrap();
            remote.logged_in = true;
            remote.loaded_sent = true;
            remote.joined = true;
            remote.first_passenger = true;
            remote.entity_id = 42;
            remote.level_load.finish();
            game.screen = Screen::Playing;
            game.streaming
                .insert_remote(&mut game.world, sunrise_client::chunk::Chunk::empty(0, 0));
            let stone = game
                .pack
                .assets
                .registry
                .default_state("minecraft:stone")
                .unwrap();
            // A floor under the rider and a wall a hair beyond it at +z.
            for x in 6..=11 {
                for z in 6..=11 {
                    game.world.set(x, 4, z, stone).unwrap();
                }
            }
            for y in 5..=7 {
                game.world.set(8, y, 9, stone).unwrap();
            }
            game.boats
                .push(Boat::new(71, "oak".into(), Vec3::new(2.5, 5.0, 2.5), 0.0));
            game.riding = Some(71);

            let mut collided = Vec::new();
            for yaw in [0.0, 180.0] {
                let mut player = PlayerMotion::new(game.motion, Vec3::new(8.5, 5.0, 8.69)).unwrap();
                player.client_side = true;
                // The player's own copy of the look is the opposite one: only this tick's look,
                // synced before the rider moves, turns it to the wall or away from it.
                player.yaw = 180.0 - yaw;
                game.player = Some(player);
                game.yaw = yaw;
                game.keys.clear();
                game.keys.insert(KeyCode::KeyW);
                game.tick_local_player(Vec::new(), Vec3::ZERO, 7).unwrap();
                let player = game.player.as_ref().unwrap();
                collided.push(player.collision.horizontal);
                assert!(
                    player.ground.on_ground,
                    "the supported move's ground survives the seating, yaw {yaw}"
                );
            }
            assert_eq!(collided, [true, false]);
            server.shutdown();
        });
    });
}
