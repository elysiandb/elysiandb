import { Outlet } from "react-router-dom";
import NavBar from "../components/header/NavBar.jsx";

export default function AdminLayout() {
    return (
        <>
            <NavBar />
            <div className="container mt-5">
                <Outlet />
            </div>
        </>
    );
}
