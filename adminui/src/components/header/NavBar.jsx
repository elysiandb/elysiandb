import { Button, Container, Nav, Navbar } from "react-bootstrap";
import { NavLink } from "react-router-dom";
import { useAuth } from "../../hooks/account/useAuth.ts";
import { useTranslation } from "../../hooks/i18n/useTranslation.js";
import LanguageSelector from "../i18n/LanguageSelector.jsx";

export default function NavBar() {
    const { logout, account } = useAuth();
    const { t } = useTranslation();

    return (
        <Navbar expand="xl" className="bg-body-tertiary">
            <div className="container-fluid">
                <Navbar.Brand as={NavLink} to="/admin">
                    ElysianDB Admin
                </Navbar.Brand>

                <Navbar.Toggle aria-controls="basic-navbar-nav" label={t("nav.toggle")} />
                <Navbar.Collapse id="basic-navbar-nav">
                    <Nav className="me-auto">

                        <Nav.Link
                            as={NavLink}
                            to="/admin"
                            end
                        >
                            {t("nav.dashboard")}
                        </Nav.Link>

                        <Nav.Link
                            as={NavLink}
                            to="/admin/monitoring"
                            end
                        >
                            {t("nav.monitoring")}
                        </Nav.Link>

                        <Nav.Link
                            as={NavLink}
                            to="/admin/logs"
                            end
                        >
                            {t("nav.logs")}
                        </Nav.Link>

                        <Nav.Link
                            as={NavLink}
                            to="/admin/configuration"
                            end
                        >
                            {t("nav.configuration")}
                        </Nav.Link>

                        <Nav.Link
                            as={NavLink}
                            to="/admin/users"
                            end
                        >
                            {t("nav.users")}
                        </Nav.Link>

                        <Nav.Link
                            as={NavLink}
                            to="/admin/acl"
                            end
                        >
                            {t("nav.acl")}
                        </Nav.Link>

                        <Nav.Link
                            as={NavLink}
                            to="/admin/schema"
                            end
                        >
                            {t("nav.schema")}
                        </Nav.Link>

                        <Nav.Link
                            as={NavLink}
                            to="/admin/hooks"
                            end
                        >
                            {t("nav.hooks")}
                        </Nav.Link>

                        <Nav.Link
                            as={NavLink}
                            to="/admin/browse"
                            end
                        >
                            {t("nav.browse")}
                        </Nav.Link>

                    </Nav>

                    <Nav className="navbar-account align-items-xl-center gap-3">
                        <LanguageSelector />
                        <span className="navbar-hello">{t("nav.hello", { username: account?.username })}</span>
                        <Button onClick={logout}>{t("nav.logout")}</Button>
                    </Nav>
                </Navbar.Collapse>
            </div>
        </Navbar>
    );
}
