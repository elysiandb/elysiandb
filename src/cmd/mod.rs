mod backup;
mod backup_remote;
mod change_password;
#[allow(clippy::module_inception)]
mod cmd;
mod create_user;
mod delete_user;
mod export_legacy;
mod http_client;
mod reset;
mod restore;

pub use backup::{backup, run_backup};
pub use backup_remote::{REMOTE_PASSWORD_ENV, runs_without_storage};
pub use change_password::change_password;
pub use cmd::{
    BACKUP_COMMAND, CHANGE_PASSWORD_COMMAND, CREATE_USER_COMMAND, DELETE_USER_COMMAND,
    END_OF_INPUT, EXPORT_LEGACY_COMMAND, ExitHook, HELP_COMMAND, Handler, Input, Output,
    PrintfHook, RESET_COMMAND, RESTORE_COMMAND, ReadPasswordHook, SERVER_COMMAND, exit, flag_args,
    get_available_commands, get_handlers, print, print_help, printf, println, read_password,
    set_exit, set_flag_args, set_os_args, set_printf, set_read_password, set_stdin, set_stdout,
    terminal_password,
};
pub use create_user::create_user;
pub use delete_user::delete_user;
pub use export_legacy::{close_storage, export_legacy, run_export_legacy};
pub use reset::reset_all;
pub use restore::{ERR_RESTORE_ABORTED, RestoreAborted, restore, run_restore};
