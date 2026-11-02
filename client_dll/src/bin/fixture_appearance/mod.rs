//! Owned, physics/item-shaped memory for external READ tests. No engine assets.
use client_dll::profile;
fn addr<const N: usize>(bytes: &[u8; N]) -> u32 {
    bytes.as_ptr() as usize as u32
}
fn put(bytes: &mut [u8], at: usize, value: u32) {
    bytes[at..at + 4].copy_from_slice(&value.to_le_bytes());
}
pub struct Fixture {
    root: Box<[u8; 4]>,
    context: Box<[u8; 0x150]>,
    world: Box<[u8; 0x4620]>,
    player: Box<[u8; 16]>,
    item: Box<[u8; 0x64]>,
    car: Box<[u8; profile::CAR_SIZE as usize]>,
    physics: Box<[u8; 0x3e8]>,
    body: Box<[u8; 64]>,
    scene: Box<[u8; 16]>,
    vtable: Box<[u8; 4]>,
    record: Box<[u8; 32]>,
    psm_item: Box<[u8; 0x68]>,
    psm_data: Box<[u8; 0x400]>,
    info: Box<[u8; 16]>,
    model: Box<[u8; 0xa8]>,
    shader: Box<[u8; 0xa8]>,
    resource_data: Box<[u8; 16]>,
    operations: Box<[u8; 40]>,
}
impl Fixture {
    pub fn new() -> Self {
        let mut f = Self {
            root: Box::new([0; 4]),
            context: Box::new([0; 0x150]),
            world: Box::new([0; 0x4620]),
            player: Box::new([0; 16]),
            item: Box::new([0; 0x64]),
            car: Box::new([0; profile::CAR_SIZE as usize]),
            physics: Box::new([0; 0x3e8]),
            body: Box::new([0; 64]),
            scene: Box::new([0; 16]),
            vtable: Box::new([0; 4]),
            record: Box::new([0; 32]),
            psm_item: Box::new([0; 0x68]),
            psm_data: Box::new([0; 0x400]),
            info: Box::new([0; 16]),
            model: Box::new([0; 0xa8]),
            shader: Box::new([0; 0xa8]),
            resource_data: Box::new([0; 16]),
            operations: Box::new([0; 40]),
        };
        put(&mut f.root[..], 0, addr(&f.context));
        put(&mut f.context[..], 0x14c, addr(&f.world));
        put(&mut f.world[..], 0x4610, addr(&f.player));
        put(&mut f.player[..], 8, addr(&f.item));
        put(&mut f.item[..], 8, addr(&f.car));
        put(&mut f.item[..], 0x34, addr(&f.record));
        put(&mut f.item[..], 0x48, profile::CAR_SIZE);
        put(&mut f.car[..], 0, addr(&f.vtable));
        put(&mut f.car[..], 0x60, addr(&f.item));
        put(&mut f.car[..], 0x84, addr(&f.scene));
        put(&mut f.car[..], 0x1d0, addr(&f.physics));
        put(&mut f.physics[..], 0x3e4, addr(&f.body));
        f.names(false);
        f
    }
    pub fn descriptors(&self) -> [u32; 3] {
        [addr(&self.root), addr(&self.vtable), addr(&self.record)]
    }
    pub fn resource_ops(&self) -> u32 {
        addr(&self.operations)
    }
    fn resources(&mut self, alternate: bool) {
        put(&mut self.car[..], 0x314, addr(&self.psm_item));
        put(&mut self.psm_item[..], 0, 0x8a);
        put(&mut self.psm_item[..], 4, 22);
        put(&mut self.psm_item[..], 8, addr(&self.psm_data));
        put(&mut self.psm_item[..], 0x30, addr(&self.operations));
        put(&mut self.psm_item[..], 0x48, 0x400);
        put(&mut self.psm_item[..], 0x4c, 1);
        put(&mut self.psm_data[..], 4, 1);
        put(&mut self.psm_data[..], 8, addr(&self.info));
        let data = addr(&self.psm_data);
        put(&mut self.psm_data[..], 0x20, data + 0x100);
        put(&mut self.psm_data[..], 0x30, data + 0x110);
        put(&mut self.psm_data[..], 0x100, addr(&self.shader));
        put(&mut self.psm_data[..], 0x110, data + 0x200);
        put(&mut self.psm_data[..], 0x200, 1);
        put(&mut self.psm_data[..], 0x204, addr(&self.psm_item));
        put(&mut self.info[..], 0, 1);
        put(&mut self.info[..], 4, addr(&self.model));
        for (item, name) in [
            (&mut self.model, b"makes/truck/own/cabin.psm".as_slice()),
            (
                &mut self.shader,
                if alternate {
                    b"makes/truck/own/blue.shd".as_slice()
                } else {
                    b"makes/truck/own/red.shd".as_slice()
                },
            ),
        ] {
            put(&mut item[..], 0, 0x448);
            put(&mut item[..], 4, 33);
            put(&mut item[..], 8, addr(&self.resource_data));
            put(&mut item[..], 0x30, addr(&self.operations));
            put(&mut item[..], 0x4c, 1);
            item[0x68..].fill(0);
            item[0x68..0x68 + name.len()].copy_from_slice(name);
        }
    }
    fn names(&mut self, alternate: bool) {
        let at = profile::APPEARANCE_MAKE as usize;
        self.car[at..at + profile::APPEARANCE_MAKE_BYTES].fill(0);
        let make = if alternate { "own_alt" } else { "own" };
        self.car[at..at + make.len()].copy_from_slice(make.as_bytes());
        let at = profile::APPEARANCE_NAMES as usize;
        self.car[at..at + 3 * profile::APPEARANCE_STRIDE].fill(0);
        for (i, name) in [
            "own_wheels",
            "own_engine",
            if alternate { "paint_blue" } else { "paint_red" },
        ]
        .iter()
        .enumerate()
        {
            let at = at + i * profile::APPEARANCE_STRIDE;
            self.car[at..at + name.len()].copy_from_slice(name.as_bytes());
        }
        self.car[profile::APPEARANCE_COUNT as usize] = 3;
        self.resources(alternate);
    }
    pub fn command(&mut self, line: &str) -> bool {
        match line {
            "appearance-switch" => self.names(true),
            "appearance-normal" => {
                self.names(false);
                put(&mut self.root[..], 0, addr(&self.context));
            }
            "appearance-missing" => put(&mut self.root[..], 0, 0),
            "appearance-count-invalid" => self.car[profile::APPEARANCE_COUNT as usize] = 65,
            "appearance-resource-invalid" => put(&mut self.psm_data[..], 0x20, 0),
            _ => return false,
        }
        true
    }
}
