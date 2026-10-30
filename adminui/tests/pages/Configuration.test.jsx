import { act, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import Configuration from "../../src/pages/Configuration.tsx";
import { useLanguageStore } from "../../src/i18n/store.js";
import { expectNoRawTranslationKeys, mockApi, renderWithProviders, requestsTo } from "../helpers.jsx";

const CONFIG = {
    Store: { Folder: "/data", Shards: 64, FlushIntervalSeconds: 5, CrashRecovery: { Enabled: true, MaxLogMB: 100 } },
    Server: {
        HTTP: { Enabled: true, Host: "0.0.0.0", Port: 8089 },
        TCP: { Enabled: false, Host: "127.0.0.1", Port: 8088 },
    },
    Log: { FlushIntervalSeconds: 10 },
    Security: { Authentication: { Enabled: true, Mode: "user", Token: "" } },
    Stats: { Enabled: false },
    Api: {
        Index: { Workers: 4 },
        Cache: { Enabled: true, CleanupIntervalSeconds: 30 },
        Schema: { Enabled: false, Strict: true },
    },
    AdminUI: { Enabled: true },
};

describe("Configuration", () => {
    it("is displayed in English", async () => {
        mockApi({ "GET /config": CONFIG });
        renderWithProviders(<Configuration />);

        expect(await screen.findByText("Folder: /data")).toBeInTheDocument();
        for (const title of ["Store", "Server", "Logging", "Security", "Stats", "API", "Admin UI"]) {
            expect(screen.getByText(title)).toBeInTheDocument();
        }
        for (const text of [
            "Shards: 64",
            "Flush Interval: 5s",
            "Flush Interval: 10s",
            "Crash Recovery: true",
            "MaxLogMB: 100",
            "HTTP: Enabled",
            "Host: 0.0.0.0",
            "Port: 8089",
            "TCP: Disabled",
            "Host: 127.0.0.1",
            "Port: 8088",
            "Authentication Enabled: true",
            "Mode: user",
            "Enabled: false",
            "Index Workers: 4",
            "Cache: Enabled",
            "Cleanup Interval: 30s",
            "Schema: Disabled",
            "Strict: true",
            "Enabled: true",
        ]) {
            expect(screen.getByText(text)).toBeInTheDocument();
        }
        expectNoRawTranslationKeys();
    });

    it("is displayed in French", async () => {
        mockApi({ "GET /config": CONFIG });
        renderWithProviders(<Configuration />, { language: "fr" });

        expect(await screen.findByText("Dossier : /data")).toBeInTheDocument();
        for (const title of ["Stockage", "Serveur", "Journalisation", "Sécurité", "Statistiques", "API", "Interface d'administration"]) {
            expect(screen.getByText(title)).toBeInTheDocument();
        }
        for (const text of [
            "Shards : 64",
            "Intervalle d'écriture : 5s",
            "Intervalle d'écriture : 10s",
            "Reprise après incident : true",
            "Taille max. du journal (Mo) : 100",
            "HTTP : Activé",
            "Hôte : 0.0.0.0",
            "Port : 8089",
            "TCP : Désactivé",
            "Hôte : 127.0.0.1",
            "Port : 8088",
            "Authentification activée : true",
            "Mode : user",
            "Activé : false",
            "Workers d'indexation : 4",
            "Cache : Activé",
            "Intervalle de nettoyage : 30s",
            "Schéma : Désactivé",
            "Strict : true",
            "Activé : true",
        ]) {
            expect(screen.getByText(text)).toBeInTheDocument();
        }
        expectNoRawTranslationKeys();
    });

    it("translates the error message", async () => {
        mockApi({ "GET /config": new Response(null, { status: 500 }) });
        renderWithProviders(<Configuration />, { language: "fr" });

        expect(await screen.findByRole("alert")).toHaveTextContent("Erreur :");
    });

    it("switches language without reloading the configuration", async () => {
        const fetchMock = mockApi({ "GET /config": CONFIG });
        renderWithProviders(<Configuration />);
        await screen.findByText("Folder: /data");

        act(() => useLanguageStore.getState().setLanguage("fr"));

        expect(screen.getByText("Dossier : /data")).toBeInTheDocument();
        expect(screen.queryByText("Folder: /data")).not.toBeInTheDocument();
        expect(requestsTo(fetchMock, "GET", "/config")).toHaveLength(1);
    });
});
