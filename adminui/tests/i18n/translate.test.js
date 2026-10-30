import { describe, expect, it } from "vitest";
import { translate } from "../../src/i18n/translate.js";

describe("translate", () => {
    it("returns the English text", () => {
        expect(translate("en", "login.submit")).toBe("Log in");
        expect(translate("en", "common.username")).toBe("Username");
    });

    it("returns the French text", () => {
        expect(translate("fr", "login.submit")).toBe("Se connecter");
        expect(translate("fr", "common.username")).toBe("Nom d'utilisateur");
    });

    it("interpolates parameters", () => {
        expect(translate("en", "nav.hello", { username: "alice" })).toBe("Hello alice");
        expect(translate("fr", "nav.hello", { username: "alice" })).toBe("Bonjour alice");
        expect(translate("fr", "acl.updated", { username: "bob", count: 3 }))
            .toBe("ACL mises à jour pour « bob » (entités : 3)");
    });

    it("replaces every occurrence of a placeholder", () => {
        expect(translate("en", "configuration.entry", { label: "Port", value: 8089 })).toBe("Port: 8089");
        expect(translate("fr", "configuration.entry", { label: "Port", value: 8089 })).toBe("Port : 8089");
    });

    it("renders null and undefined parameters as empty strings", () => {
        expect(translate("en", "nav.hello", { username: undefined })).toBe("Hello ");
        expect(translate("en", "nav.hello", { username: null })).toBe("Hello ");
    });

    it("renders falsy but meaningful parameters", () => {
        expect(translate("en", "configuration.entry", { label: "Shards", value: 0 })).toBe("Shards: 0");
        expect(translate("en", "configuration.entry", { label: "Strict", value: false })).toBe("Strict: false");
    });

    it("keeps placeholders that have no matching parameter", () => {
        expect(translate("en", "nav.hello")).toBe("Hello {username}");
        expect(translate("en", "nav.hello", { other: "x" })).toBe("Hello {username}");
    });

    it("does not resolve placeholders from the object prototype", () => {
        expect(translate("en", "nav.hello", Object.create({ username: "inherited" }))).toBe("Hello {username}");
    });

    it("falls back to English for an unsupported language", () => {
        expect(translate("de", "login.submit")).toBe("Log in");
        expect(translate(undefined, "login.submit")).toBe("Log in");
    });

    it("returns the key when it does not exist", () => {
        expect(translate("fr", "does.not.exist")).toBe("does.not.exist");
    });

    it("returns the key when it points to a group instead of a text", () => {
        expect(translate("en", "common")).toBe("common");
        expect(translate("en", "acl.permissions")).toBe("acl.permissions");
    });
});
