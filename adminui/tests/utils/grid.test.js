import { describe, expect, it } from "vitest";
import { fullRowColumns } from "../../src/utils/grid.js";

const BREAKPOINTS = ["xs", "sm", "md", "lg", "xl", "xxl"];

function spanAt(columns, breakpoint) {
    for (let index = BREAKPOINTS.indexOf(breakpoint); index >= 0; index--) {
        if (columns[BREAKPOINTS[index]] !== undefined) {
            return columns[BREAKPOINTS[index]];
        }
    }
    return 12;
}

describe("fullRowColumns", () => {
    it.each(Array.from({ length: 12 }, (_, index) => index + 1))("fills every line with %i cards", (count) => {
        const columns = fullRowColumns(count);

        for (const breakpoint of BREAKPOINTS) {
            const span = spanAt(columns, breakpoint);
            expect(12 % span, breakpoint).toBe(0);
            expect((count * span) % 12, breakpoint).toBe(0);
        }
    });

    it("puts several cards side by side on wide screens", () => {
        expect(fullRowColumns(2).lg).toBe(6);
        expect(fullRowColumns(3).lg).toBe(4);
        expect(fullRowColumns(4).lg).toBe(6);
        expect(fullRowColumns(6)).toEqual({ xs: 12, lg: 6, xxl: 4 });
    });
});
