import {Account} from "../account/Account";

export class AccountStorage {
  static persist(account: Account) {
    localStorage.setItem("account", JSON.stringify(account));
  }

  static destroy() {
    localStorage.removeItem("account");
  }

  static retrieve(): Account | null {
    const data = localStorage.getItem("account");
    if (data === null) {
      return null;
    }
    return JSON.parse(data) as Account;
  }
}
