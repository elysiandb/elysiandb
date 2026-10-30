import {FormEventHandler} from "react";
import {useAuth} from "../hooks/account/useAuth";
import {useTranslation} from "../hooks/i18n/useTranslation.js";
import LanguageSelector from "../components/i18n/LanguageSelector.jsx";

export function Login() {
  const { login } = useAuth();
  const { t } = useTranslation();
  const handleSubmit: FormEventHandler<HTMLFormElement> = (e) => {
    e.preventDefault();
    const data = new FormData(e.currentTarget);
    login(data.get("username")!.toString(), data.get("password")!.toString());
  };

  return (
    <>
      <div className="d-flex justify-content-end mb-3">
        <LanguageSelector />
      </div>
      <form onSubmit={handleSubmit}>
        <div className="mb-3">
          <label htmlFor="login-username" className="form-label">
            {t("common.username")}
          </label>
          <input
            id="login-username"
            defaultValue="admin"
            type="text"
            className="form-control"
            name="username"
          />
        </div>
        <div className="mb-3">
          <label htmlFor="login-password" className="form-label">
            {t("common.password")}
          </label>
          <input
            id="login-password"
            defaultValue="admin"
            type="password"
            className="form-control"
            name="password"
          />
        </div>
        <button className="btn btn-primary">{t("login.submit")}</button>
      </form>
    </>
  );
}
