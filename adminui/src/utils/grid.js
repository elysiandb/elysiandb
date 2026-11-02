export function fullRowColumns(count) {
    if (count % 6 === 0) {
        return { xs: 12, lg: 6, xxl: 4 };
    }
    if (count % 3 === 0) {
        return { xs: 12, lg: 4 };
    }
    if (count % 2 === 0) {
        return { xs: 12, lg: 6 };
    }
    return { xs: 12 };
}
