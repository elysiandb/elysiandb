export type ElysianConfiguration = {
    Store: {
        Folder: string;
        Shards: number;
        FlushIntervalSeconds: number;
        CrashRecovery: {
            Enabled: boolean;
            MaxLogMB: number;
        };
    };

    Server: {
        HTTP: {
            Enabled: boolean;
            Host: string;
            Port: number;
        };
        TCP: {
            Enabled: boolean;
            Host: string;
            Port: number;
        };
    };

    Log: {
        FlushIntervalSeconds: number;
    };

    Security: {
        Authentication: {
            Enabled: boolean;
            Mode: string;
            Token: string;
        };
    };

    Stats: {
        Enabled: boolean;
    };

    Api: {
        Index: {
            Workers: number;
        };
        Cache: {
            Enabled: boolean;
            CleanupIntervalSeconds: number;
        };
        Schema: {
            Enabled: boolean;
            Strict: boolean;
        };
    };

    AdminUI: {
        Enabled: boolean;
    };
};
