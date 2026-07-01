/// User table belongs to the shell/init layer, not the core kernel.
/// The kernel only deals with uid/gid integers; name-to-uid resolution lives here.

const MAX_USERS: usize = 8;

#[derive(Clone, Copy)]
pub struct User {
    pub uid:      u16,
    pub gid:      u16,
    pub used:     bool,
    name:         [u8; 32],
    name_len:     usize,
}

impl User {
    const fn empty() -> Self {
        Self { uid: 0, gid: 0, used: false, name: [0u8; 32], name_len: 0 }
    }
    pub fn name_str(&self) -> &str {
        core::str::from_utf8(&self.name[..self.name_len]).unwrap_or("?")
    }
}

static mut USERS: [User; MAX_USERS] = [const { User::empty() }; MAX_USERS];
static mut COUNT: usize = 0;

pub fn init() {
    add(0,    0,    "root");
    add(1000, 1000, "ferrugem");
}

fn add(uid: u16, gid: u16, name: &str) {
    unsafe {
        if COUNT >= MAX_USERS { return; }
        let i = COUNT;
        let b = name.as_bytes();
        let n = b.len().min(32);
        USERS[i].uid = uid;
        USERS[i].gid = gid;
        USERS[i].used = true;
        USERS[i].name[..n].copy_from_slice(&b[..n]);
        USERS[i].name_len = n;
        COUNT += 1;
    }
}

pub fn lookup_by_name(name: &str) -> Option<&'static User> {
    unsafe {
        for i in 0..COUNT {
            if USERS[i].used && USERS[i].name_str() == name {
                return Some(&USERS[i]);
            }
        }
        None
    }
}

pub fn lookup_by_uid(uid: u16) -> Option<&'static User> {
    unsafe {
        for i in 0..COUNT {
            if USERS[i].used && USERS[i].uid == uid {
                return Some(&USERS[i]);
            }
        }
        None
    }
}

pub fn list() -> &'static [User] {
    unsafe { &USERS[..COUNT] }
}
