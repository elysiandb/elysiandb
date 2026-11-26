use crate::acl;

pub fn boot_acl() {
    let _ = acl::init_acl();
}
