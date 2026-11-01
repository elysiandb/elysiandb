export const PROBLEMS = {
    flush_failing: { message: "status.problems.flush_failing", link: "/admin/logs?component=storage&level=error", linkLabel: "nav.logs" },
    recovery_write_failing: { message: "status.problems.recovery_write_failing", link: "/admin/logs?component=recovery&level=error", linkLabel: "nav.logs" },
    mongodb_unreachable: { message: "status.problems.mongodb_unreachable", link: "/admin/logs?component=mongodb", linkLabel: "nav.logs" },
    error_rate_high: { message: "status.problems.error_rate_high", link: "/admin/logs?component=http&level=error", linkLabel: "nav.logs" },
    latency_high: { message: "status.problems.latency_high", link: "/admin/monitoring", linkLabel: "nav.monitoring" },
    hook_failing: { message: "status.problems.hook_failing", link: "/admin/hooks", linkLabel: "nav.hooks" },
    auth_failures: { message: "status.problems.auth_failures", link: "/admin/logs?component=security&level=warn", linkLabel: "nav.logs" },
    index_backlog: { message: "status.problems.index_backlog", link: "/admin/monitoring", linkLabel: "nav.monitoring" },
    old_transaction: { message: "status.problems.old_transaction", link: "/admin/monitoring", linkLabel: "nav.monitoring" },
    default_admin_password: { message: "status.problems.default_admin_password", link: "/admin/users", linkLabel: "nav.users" },
    auth_disabled_public: { message: "status.problems.auth_disabled_public", link: "/admin/configuration", linkLabel: "nav.configuration" },
    unclean_shutdown: { message: "status.problems.unclean_shutdown", link: "/admin/logs?component=recovery", linkLabel: "nav.logs" },
};

export const STATUS_LEVELS = {
    healthy: { label: "status.healthy", variant: "success" },
    warning: { label: "status.warning", variant: "warning" },
    critical: { label: "status.critical", variant: "danger" },
};

export const SEVERITIES = {
    critical: { label: "status.severity.critical", variant: "danger" },
    warning: { label: "status.severity.warning", variant: "warning" },
    info: { label: "status.severity.info", variant: "info" },
};

export function problemParams(problem, format) {
    const details = problem?.details ?? {};

    return {
        code: problem?.code ?? "",
        hook_id: details.hook_id ?? "?",
        remote_addr: details.remote_addr ?? "?",
        duration: format.duration(details.seconds),
        entries: format.count(details.entries),
    };
}
