import { Button, Container, Nav, Navbar } from "react-bootstrap";
import { NavLink } from "react-router-dom";
import { useAuth } from "../../hooks/account/useAuth.ts";

export default function NavBar() {
    const { logout, account } = useAuth();

    return (
        <Navbar expand="lg" className="bg-body-tertiary">
            <div className="container-fluid">
                <Navbar.Brand as={NavLink} to="/admin">
                    ElysianDB Admin
                </Navbar.Brand>

                <Navbar.Toggle aria-controls="basic-navbar-nav" />
                <Navbar.Collapse id="basic-navbar-nav">
                    <Nav className="me-auto">

                        <Nav.Link
                            as={NavLink}
                            to="/admin/configuration"
                            end
                        >
                            Configuration
                        </Nav.Link>

                        <Nav.Link
                            as={NavLink}
                            to="/admin/users"
                            end
                        >
                            Users
                        </Nav.Link>

                        <Nav.Link
                            as={NavLink}
                            to="/admin/acl"
                            end
                        >
                            ACL
                        </Nav.Link>

                        <Nav.Link
                            as={NavLink}
                            to="/admin/schema"
                            end
                        >
                            Schema
                        </Nav.Link>

                        <Nav.Link
                            as={NavLink}
                            to="/admin/hooks"
                            end
                        >
                            Hooks
                        </Nav.Link>

                        <Nav.Link
                            as={NavLink}
                            to="/admin/browse"
                            end
                        >
                            Browse
                        </Nav.Link>

                    </Nav>
                </Navbar.Collapse>

                <Nav className="align-items-center gap-3">
                    <span className="navbar-hello">Hello {account?.username}</span>
                    <Button onClick={logout}>Logout</Button>
                </Nav>
            </div>
        </Navbar>
    );
}
