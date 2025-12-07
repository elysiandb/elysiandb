import { Button, Container, Nav, Navbar } from "react-bootstrap";
import { NavLink } from "react-router-dom";
import { useAuth } from "../../hooks/account/useAuth.ts";

export default function NavBar() {
    const { logout } = useAuth();

    return (
        <Navbar expand="lg" className="bg-body-tertiary">
            <Container>
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
                            to="/admin/entities"
                            end
                        >
                            Entities
                        </Nav.Link>

                    </Nav>
                </Navbar.Collapse>

                <Nav>
                    <Button onClick={logout}>Logout</Button>
                </Nav>
            </Container>
        </Navbar>
    );
}
