import { create } from "zustand";
import { combine, persist } from "zustand/middleware";
import { Account } from "./Account.ts";

export const useAccountStore = create(
  persist(
    combine(
      {
        account: null as undefined | null | Account,
      },
      (set) => ({
        setAccount: (account: Account | null) => set({ account }),
      })
    ),
    { name: "account" }
  )
);
