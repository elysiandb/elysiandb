import {useAccountStore} from "../account/useAccounttStore.ts";

export function useAccount() {
    const { account } = useAccountStore();

    if (!account) {
        throw new Error("User is not authenticated");
    }

    return {
        account,
    };
}