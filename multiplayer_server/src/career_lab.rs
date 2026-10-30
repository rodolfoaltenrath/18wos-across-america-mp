//! Reproducible offline end-to-end exercise using only owned synthetic data.
//! Runs the actual store, TCP server/client and ledger; not the commercial engine.
use crate::{
    career::{self, Compatibility, Goal},
    career_net::{self, Invite},
    career_store as store,
    career_world::{Operation as O, Outcome, WorldStore},
    identity,
    protocol::{Position, Rotation},
    session_protocol,
};
use anyhow::{Result, ensure};
use serde::Serialize;
use std::{fs, path::Path};

#[derive(Debug, Serialize)]
pub struct Report {
    pub schema: u32,
    pub iterations: u32,
    pub durable_operations: u64,
    pub duplicate_receipts: u64,
    pub refused_operations: u64,
    pub restart_recoveries: u64,
    pub tcp_bootstraps: u64,
    pub tcp_world_snapshots: u64,
    pub tcp_refusals: u64,
    pub original_fixture_preserved: bool,
    pub game_tested: bool,
    pub two_pc_lan_tested: bool,
}
fn id(n: u64) -> String {
    format!("{n:032x}")
}
fn execute(world: &mut WorldStore, id_number: u64, op: O) -> Result<()> {
    ensure!(
        !world
            .execute(&id(id_number), &op)
            .map_err(anyhow::Error::msg)?
            .duplicate,
        "unexpected duplicate"
    );
    Ok(())
}
pub async fn run(output: &Path, iterations: u32) -> Result<Report> {
    ensure!(
        (1..=1000).contains(&iterations),
        "lab iterations outside 1..1000"
    );
    fs::create_dir(output)?;
    let mut report = Report {
        schema: 1,
        iterations,
        durable_operations: 0,
        duplicate_receipts: 0,
        refused_operations: 0,
        restart_recoveries: 0,
        tcp_bootstraps: 0,
        tcp_world_snapshots: 0,
        tcp_refusals: 0,
        original_fixture_preserved: true,
        game_tested: false,
        two_pc_lan_tested: false,
    };
    for iteration in 0..iterations {
        let case = output.join(format!("case-{iteration:04}"));
        fs::create_dir(&case)?;
        let source = case.join("own-fixture.sav");
        let bytes = vec![42; 16384];
        store::write_new(&source, &bytes).map_err(anyhow::Error::msg)?;
        let compatibility = Compatibility {
            profile: session_protocol::PROFILE.into(),
            map_sha256: career::hash(b"own map fixture"),
            mods_sha256: vec![],
        };
        let data = case.join("host");
        let manifest = store::create(&data, &source, Goal::Unlimited, compatibility.clone())
            .map_err(anyhow::Error::msg)?;
        let invite = Invite::new(&manifest)?;
        store::write_new(
            &data.join("invite.json"),
            &store::json(&invite).map_err(anyhow::Error::msg)?,
        )
        .map_err(anyhow::Error::msg)?;
        let mut world = WorldStore::open(&data).map_err(anyhow::Error::msg)?;
        execute(
            &mut world,
            1,
            O::Register {
                player: id(1),
                name: "Transportadora A".into(),
            },
        )?;
        execute(
            &mut world,
            2,
            O::Register {
                player: id(2),
                name: "Transportadora B".into(),
            },
        )?;
        execute(
            &mut world,
            3,
            O::Offer {
                contract: id(100),
                origin: "miami".into(),
                destination: "houston".into(),
                reward_minor: 2500,
            },
        )?;
        let accept = O::Accept {
            player: id(1),
            contract: id(100),
        };
        execute(&mut world, 4, accept.clone())?;
        ensure!(
            world
                .execute(
                    &id(5),
                    &O::Accept {
                        player: id(2),
                        contract: id(100)
                    }
                )
                .is_err(),
            "contract double-accepted"
        );
        report.refused_operations += 1;
        let deliver = O::Deliver {
            player: id(1),
            contract: id(100),
            host_attestation_sha256: career::hash(b"lab host attestation; NOT engine evidence"),
        };
        ensure!(
            world.execute(&id(6), &deliver).is_err(),
            "delivery without checkpoint"
        );
        report.refused_operations += 1;
        execute(
            &mut world,
            7,
            O::Checkpoint {
                player: id(1),
                vehicle: "truck_fixture".into(),
                position: Position {
                    x: 100.,
                    y: 200.,
                    z: 300.,
                },
                rotation: Rotation {
                    x: 0.,
                    y: 0.,
                    z: 0.,
                    w: 1.,
                },
            },
        )?;
        drop(world);
        let mut world = WorldStore::open(&data).map_err(anyhow::Error::msg)?;
        report.restart_recoveries += 1;
        ensure!(
            world
                .execute(&id(4), &accept)
                .map_err(anyhow::Error::msg)?
                .duplicate,
            "accept retry was replayed"
        );
        report.duplicate_receipts += 1;
        let receipt = world
            .execute(&id(6), &deliver)
            .map_err(anyhow::Error::msg)?;
        ensure!(
            receipt.outcome
                == Outcome::Paid {
                    reward_minor: 2500,
                    balance_minor: 2500
                },
            "wrong payment"
        );
        drop(world); // Simulates lost acknowledgement after durable commit.
        let mut world = WorldStore::open(&data).map_err(anyhow::Error::msg)?;
        report.restart_recoveries += 1;
        ensure!(
            world
                .execute(&id(6), &deliver)
                .map_err(anyhow::Error::msg)?
                .duplicate,
            "payment retry was replayed"
        );
        report.duplicate_receipts += 1;
        ensure!(
            world.execute(&id(8), &deliver).is_err(),
            "payment with a new ID was repeated"
        );
        report.refused_operations += 1;
        ensure!(
            world
                .execute(
                    &id(6),
                    &O::ConfigureClock {
                        paused: true,
                        scale: 1
                    }
                )
                .is_err(),
            "operation ID reused"
        );
        report.refused_operations += 1;
        world.advance(1234).map_err(anyhow::Error::msg)?;
        execute(
            &mut world,
            9,
            O::ConfigureClock {
                paused: true,
                scale: 60,
            },
        )?;
        world.advance(60_000).map_err(anyhow::Error::msg)?;
        ensure!(
            world.snapshot().clock.world_ms == 1234,
            "paused clock advanced"
        );
        report.durable_operations += 7;
        drop(world);

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
        let endpoint = listener.local_addr()?;
        let server_data = data.clone();
        let (stop, stopped) = tokio::sync::oneshot::channel();
        let server = tokio::spawn(async move {
            career_net::serve(listener, &server_data, async {
                let _ = stopped.await;
            })
            .await
        });
        let import = case.join("guest");
        let (a, b, bootstrap) = tokio::join!(
            career_net::world_status(endpoint, &invite, compatibility.clone()),
            career_net::world_status(endpoint, &invite, compatibility.clone()),
            career_net::fetch(endpoint, &invite, compatibility.clone(), &import)
        );
        let a = a?;
        let b = b?;
        let bootstrap = bootstrap?;
        ensure!(
            a.career == b.career && a.clock == b.clock && a.clock.world_ms == 1234,
            "clients observed different paused sidecar clocks"
        );
        ensure!(
            a.ranking[0].company.balance_minor == 2500
                && b.ranking[0].company.balance_minor == 2500,
            "clients observed inconsistent ledger"
        );
        ensure!(
            bootstrap.bootstrap_verified && !bootstrap.world_synchronized,
            "bad bootstrap claim"
        );
        career_net::inspect_import(&import)?;
        ensure!(
            WorldStore::open(&data).is_err(),
            "server allowed another writer"
        );
        let mut wrong = compatibility.clone();
        wrong.map_sha256 = career::hash(b"other map");
        ensure!(
            career_net::world_status(endpoint, &invite, wrong)
                .await
                .is_err(),
            "incompatible map accepted"
        );
        let mut wrong_invite = invite.clone();
        wrong_invite.secret = identity::random_id().map_err(anyhow::Error::msg)?;
        ensure!(
            career_net::fetch(
                endpoint,
                &wrong_invite,
                compatibility,
                &case.join("refused")
            )
            .await
            .is_err(),
            "bad invite accepted"
        );
        ensure!(
            !case.join("refused").exists(),
            "refusal created a destination"
        );
        stop.send(())
            .map_err(|_| anyhow::anyhow!("server stopped unexpectedly"))?;
        let server_report = server.await??;
        ensure!(
            server_report.connections == 5 && server_report.busy == 0,
            "unexpected control connections"
        );
        report.tcp_bootstraps += 1;
        report.tcp_world_snapshots += 2;
        report.tcp_refusals += 2;
        let recovered = WorldStore::open(&data).map_err(anyhow::Error::msg)?;
        report.restart_recoveries += 1;
        ensure!(
            recovered.snapshot().ranking[0].company.balance_minor == 2500,
            "ledger lost after service shutdown"
        );
        ensure!(
            store::read_bounded(&source, bytes.len()).map_err(anyhow::Error::msg)? == bytes,
            "source fixture changed"
        );
        if iteration == 0 {
            store::write_new(
                &output.join("world-example.json"),
                &store::json(&recovered.snapshot()).map_err(anyhow::Error::msg)?,
            )
            .map_err(anyhow::Error::msg)?;
        }
        drop(recovered);
        // Only directories constructed by this run inside its create_new root.
        fs::remove_dir_all(case)?;
    }
    store::write_new(
        &output.join("report.json"),
        &store::json(&report).map_err(anyhow::Error::msg)?,
    )
    .map_err(anyhow::Error::msg)?;
    Ok(report)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn full_production_sidecar_flow_without_game_or_commercial_assets() {
        let dir = crate::career_store::tests::Temp::new();
        let report = run(&dir.0.join("lab"), 2).await.unwrap();
        assert_eq!(report.durable_operations, 14);
        assert_eq!(report.duplicate_receipts, 4);
        assert_eq!(report.tcp_world_snapshots, 4);
        assert_eq!(report.restart_recoveries, 6);
        assert!(!report.game_tested && !report.two_pc_lan_tested);
        assert!(run(&dir.0.join("lab"), 1).await.is_err());
    }
}
