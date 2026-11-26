use crate::api::guard::AbandonedProbe;
use crate::httpd::ConnHandle;

pub fn disconnect_probe(_conn: Option<&ConnHandle>) -> Option<AbandonedProbe> {
    None
}
