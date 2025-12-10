import {FormEventHandler} from "react";
import {useAuth} from "../hooks/account/useAuth";

export function Login() {
  const { login } = useAuth();
  const handleSubmit: FormEventHandler<HTMLFormElement> = (e) => {
    e.preventDefault();
    const data = new FormData(e.currentTarget);
    login(data.get("username")!.toString(), data.get("password")!.toString());
  };

  return (
    <form onSubmit={handleSubmit}>
      <div className="mb-3">
        <label htmlFor="exampleInputEmail1" className="form-label">
          Nom d'utilisateur
        </label>
        <input
          defaultValue="admin"
          type="text"
          className="form-control"
          name="username"
        />
      </div>
      <div className="mb-3">
        <label htmlFor="exampleInputEmail1" className="form-label">
          Nom d'utilisateur
        </label>
        <input
          defaultValue="admin"
          type="password"
          className="form-control"
          name="password"
        />
      </div>
      <button className="btn btn-primary">Se connecter</button>
    </form>
  );
}
