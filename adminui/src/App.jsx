import 'bootstrap/dist/css/bootstrap.min.css';
import { createBrowserRouter, RouterProvider } from "react-router-dom";
import AdminLayout from "./layouts/AdminLayout.jsx";
import Home from "./pages/Home.jsx";
import Configuration from "./pages/Configuration.tsx";
import {AuthStatus, useAuth} from "./hooks/account/useAuth.ts";
import {Login} from "./pages/Login.tsx";
import EntityTypesList from "./pages/EntityTypesList.jsx";
import UsersList from "./pages/UsersList.jsx";
import ACLList from "./pages/ACLList.jsx";
import HooksList from "./pages/HooksList.jsx";
import EntitiesBrowser from "./pages/EntitiesBrowser.jsx";

export default function App() {

    const { status } = useAuth();

    if (status === AuthStatus.Unknown) {
        return (
            <div className="mx-auto my-5" style={{ width: "min-content" }}>
                <div className="spinner-border text-primary" role="status">
                    <span className="visually-hidden">Loading...</span>
                </div>
            </div>
        );
    }

    if (status === AuthStatus.Guest) {
        return (
            <div className="container mt-5">
                <Login />
            </div>
        );
    }

    const router = createBrowserRouter([
        {
            path: "/admin",
            element: <AdminLayout />,
            children: [
                {
                    index: true,
                    element: <Home />,
                },
                {
                    path: "configuration",
                    element: <Configuration />,
                },
                {
                    path: "users",
                    element: <UsersList />,
                },
                {
                    path: "schema",
                    element: <EntityTypesList />,
                },
                {
                    path: "acl",
                    element: <ACLList />,
                },
                {
                    path: "hooks",
                    element: <HooksList />,
                },
                {
                    path: "browse",
                    element: <EntitiesBrowser />,
                },
            ],
        },
    ]);

    return <RouterProvider router={router} />;
}
