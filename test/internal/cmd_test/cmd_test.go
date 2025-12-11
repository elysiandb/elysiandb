package cmd_test

import (
	"bytes"
	"fmt"
	"os"
	"testing"

	"github.com/taymour/elysiandb/internal/cmd"
	"github.com/taymour/elysiandb/internal/configuration"
	"github.com/taymour/elysiandb/internal/globals"
	"github.com/taymour/elysiandb/internal/security"
)

func stripANSI(b []byte) []byte {
	out := make([]byte, 0, len(b))
	i := 0
	for i < len(b) {
		if b[i] == 0x1b {
			i++
			for i < len(b) &&
				((b[i] >= '0' && b[i] <= '9') || b[i] == '[' || b[i] == ';') {
				i++
			}
			if i < len(b) {
				i++
			}
		} else {
			out = append(out, b[i])
			i++
		}
	}
	return out
}

func mockStdin(input string) func() {
	r, w, _ := os.Pipe()
	orig := os.Stdin
	w.Write([]byte(input))
	w.Close()
	os.Stdin = r
	return func() { os.Stdin = orig }
}

func captureCmdOutput() (*bytes.Buffer, func()) {
	var buf bytes.Buffer
	orig := cmd.Printf
	cmd.Printf = func(format string, a ...interface{}) (int, error) {
		return buf.WriteString(fmt.Sprintf(format, a...))
	}
	return &buf, func() { cmd.Printf = orig }
}

func TestDeleteUser_AuthDisabled(t *testing.T) {
	globals.SetConfig(&configuration.Config{
		Security: configuration.SecurityConfig{
			Authentication: configuration.AuthenticationConfig{
				Enabled: false,
			},
		},
	})

	buf, restore := captureCmdOutput()
	defer restore()

	cmd.DeleteUser()

	clean := stripANSI(buf.Bytes())
	if !bytes.Contains(clean, []byte("Authentication is disabled")) {
		t.Fatalf("expected message, got: %s", string(clean))
	}
}

func TestDeleteUser_WrongMode(t *testing.T) {
	globals.SetConfig(&configuration.Config{
		Security: configuration.SecurityConfig{
			Authentication: configuration.AuthenticationConfig{
				Enabled: true,
				Mode:    "token",
			},
		},
	})

	buf, restore := captureCmdOutput()
	defer restore()

	cmd.DeleteUser()

	clean := stripANSI(buf.Bytes())
	if !bytes.Contains(clean, []byte("only supports basic")) {
		t.Fatalf("expected message, got: %s", string(clean))
	}
}

func TestCreateUser_AuthDisabled(t *testing.T) {
	globals.SetConfig(&configuration.Config{
		Security: configuration.SecurityConfig{
			Authentication: configuration.AuthenticationConfig{
				Enabled: false,
			},
		},
	})

	buf, restore := captureCmdOutput()
	defer restore()

	cmd.CreateUser()

	clean := stripANSI(buf.Bytes())
	if !bytes.Contains(clean, []byte("Authentication is disabled")) {
		t.Fatalf("expected message, got: %s", string(clean))
	}
}

func TestCreateUser_WrongMode(t *testing.T) {
	globals.SetConfig(&configuration.Config{
		Security: configuration.SecurityConfig{
			Authentication: configuration.AuthenticationConfig{
				Enabled: true,
				Mode:    "token",
			},
		},
	})

	buf, restore := captureCmdOutput()
	defer restore()

	cmd.CreateUser()

	clean := stripANSI(buf.Bytes())
	if !bytes.Contains(clean, []byte("only supports basic")) {
		t.Fatalf("expected message, got: %s", string(clean))
	}
}

func TestDeleteUser_OK(t *testing.T) {
	globals.SetConfig(&configuration.Config{
		Security: configuration.SecurityConfig{
			Authentication: configuration.AuthenticationConfig{
				Enabled: true,
				Mode:    "basic",
			},
		},
		Store: configuration.StoreConfig{Folder: t.TempDir()},
	})

	security.CreateBasicUser(&security.BasicUser{Username: "john", Password: "x", Role: security.RoleUser})

	restoreIn := mockStdin("john\n")
	defer restoreIn()

	buf, restore := captureCmdOutput()
	defer restore()

	cmd.DeleteUser()

	clean := stripANSI(buf.Bytes())
	if !bytes.Contains(clean, []byte("deleted successfully")) {
		t.Fatalf("expected deletion success message, got: %s", string(clean))
	}
}

func TestDeleteUser_Unknown(t *testing.T) {
	globals.SetConfig(&configuration.Config{
		Security: configuration.SecurityConfig{
			Authentication: configuration.AuthenticationConfig{
				Enabled: true,
				Mode:    "basic",
			},
		},
		Store: configuration.StoreConfig{Folder: t.TempDir()},
	})

	restoreIn := mockStdin("ghost\n")
	defer restoreIn()

	buf, restore := captureCmdOutput()
	defer restore()

	cmd.DeleteUser()

	clean := stripANSI(buf.Bytes())
	if !bytes.Contains(clean, []byte("Failed to delete user")) {
		t.Fatalf("expected error message, got: %s", string(clean))
	}
}

func TestCreateUser_OK(t *testing.T) {
	globals.SetConfig(&configuration.Config{
		Security: configuration.SecurityConfig{
			Authentication: configuration.AuthenticationConfig{
				Enabled: true,
				Mode:    "basic",
			},
		},
		Store: configuration.StoreConfig{Folder: t.TempDir()},
	})

	restoreIn := mockStdin("alice\n1\n")
	defer restoreIn()

	cmd.ReadPassword = func(int) ([]byte, error) { return []byte("secret"), nil }

	buf, restore := captureCmdOutput()
	defer restore()

	cmd.CreateUser()

	clean := stripANSI(buf.Bytes())
	if !bytes.Contains(clean, []byte("created successfully")) {
		t.Fatalf("expected success message, got: %s", string(clean))
	}
}

func TestCreateUser_PasswordMismatch(t *testing.T) {
	cfg := &configuration.Config{}
	cfg.Security.Authentication.Enabled = true
	cfg.Security.Authentication.Mode = "basic"
	cfg.Store.Folder = t.TempDir()
	globals.SetConfig(cfg)

	input := "bob\n2\n"
	restoreIn := mockStdin(input)
	defer restoreIn()

	call := 0
	cmd.ReadPassword = func(int) ([]byte, error) {
		if call == 0 {
			call++
			return []byte("pass1"), nil
		}
		return []byte("pass2"), nil
	}

	r, w, _ := os.Pipe()
	orig := os.Stdout
	os.Stdout = w

	cmd.CreateUser()

	w.Close()
	os.Stdout = orig
	var buf bytes.Buffer
	buf.ReadFrom(r)

	clean := stripANSI(buf.Bytes())
	if !bytes.Contains(clean, []byte("Passwords do not match")) {
		t.Fatalf("expected mismatch message, got: %s", string(clean))
	}
}

func TestGetAvailableCommands(t *testing.T) {
	cmds := cmd.GetAvailableCommands()

	if cmds[cmd.ServerCommand] == "" ||
		cmds[cmd.CreateUserCommand] == "" ||
		cmds[cmd.DeleteUserCommand] == "" ||
		cmds[cmd.HelpCommand] == "" {
		t.Fatalf("expected all commands to be present")
	}
}

func TestGetHandlers(t *testing.T) {
	h := cmd.GetHandlers()

	if h[cmd.ServerCommand] == nil ||
		h[cmd.CreateUserCommand] == nil ||
		h[cmd.DeleteUserCommand] == nil ||
		h[cmd.HelpCommand] == nil {
		t.Fatalf("expected all handlers to be present")
	}
}

func TestPrintHelp(t *testing.T) {
	var buf bytes.Buffer
	orig := cmd.Printf
	cmd.Printf = func(format string, a ...interface{}) (int, error) {
		return buf.WriteString(fmt.Sprintf(format, a...))
	}
	defer func() { cmd.Printf = orig }()

	cmd.PrintHelp()

	out := stripANSI(buf.Bytes())

	expected := []string{
		cmd.ServerCommand,
		cmd.CreateUserCommand,
		cmd.DeleteUserCommand,
		cmd.HelpCommand,
	}

	for _, e := range expected {
		if !bytes.Contains(out, []byte(e)) {
			t.Fatalf("expected help output to contain %s, got: %s", e, string(out))
		}
	}
}

func TestChangePassword_AuthDisabled(t *testing.T) {
	globals.SetConfig(&configuration.Config{
		Security: configuration.SecurityConfig{
			Authentication: configuration.AuthenticationConfig{
				Enabled: false,
			},
		},
	})

	buf, restore := captureCmdOutput()
	defer restore()

	cmd.ChangePassword()

	clean := stripANSI(buf.Bytes())
	if !bytes.Contains(clean, []byte("Authentication is disabled")) {
		t.Fatalf("expected message, got: %s", string(clean))
	}
}

func TestChangePassword_WrongMode(t *testing.T) {
	globals.SetConfig(&configuration.Config{
		Security: configuration.SecurityConfig{
			Authentication: configuration.AuthenticationConfig{
				Enabled: true,
				Mode:    "token",
			},
		},
	})

	buf, restore := captureCmdOutput()
	defer restore()

	cmd.ChangePassword()

	clean := stripANSI(buf.Bytes())
	if !bytes.Contains(clean, []byte("only supports basic")) {
		t.Fatalf("expected message, got: %s", string(clean))
	}
}

func TestChangePassword_UserNotFound(t *testing.T) {
	globals.SetConfig(&configuration.Config{
		Security: configuration.SecurityConfig{
			Authentication: configuration.AuthenticationConfig{
				Enabled: true,
				Mode:    "basic",
			},
		},
		Store: configuration.StoreConfig{Folder: t.TempDir()},
	})

	restoreIn := mockStdin("ghost\n")
	defer restoreIn()

	buf, restore := captureCmdOutput()
	defer restore()

	cmd.ChangePassword()

	clean := stripANSI(buf.Bytes())

	if !bytes.Contains(clean, []byte("Failed to update password")) &&
		!bytes.Contains(clean, []byte("not found")) {
		t.Fatalf("expected not found error, got: %s", string(clean))
	}
}

func TestChangePassword_Mismatch(t *testing.T) {
	dir := t.TempDir()
	globals.SetConfig(&configuration.Config{
		Security: configuration.SecurityConfig{
			Authentication: configuration.AuthenticationConfig{
				Enabled: true,
				Mode:    "basic",
			},
		},
		Store: configuration.StoreConfig{Folder: dir},
	})

	security.CreateBasicUser(&security.BasicUser{Username: "bob", Password: "x", Role: security.RoleUser})

	restoreIn := mockStdin("bob\n")
	defer restoreIn()

	call := 0
	cmd.ReadPassword = func(int) ([]byte, error) {
		if call == 0 {
			call++
			return []byte("pass1"), nil
		}
		return []byte("pass2"), nil
	}

	_, w, _ := os.Pipe()
	origOut := os.Stdout
	origPrintf := cmd.Printf
	os.Stdout = w
	var buf bytes.Buffer
	cmd.Printf = func(format string, a ...interface{}) (int, error) {
		return buf.WriteString(fmt.Sprintf(format, a...))
	}

	cmd.ChangePassword()

	w.Close()
	os.Stdout = origOut
	cmd.Printf = origPrintf

	clean := stripANSI(buf.Bytes())
	if !bytes.Contains(clean, []byte("Passwords do not match")) {
		t.Fatalf("expected mismatch message, got: %s", string(clean))
	}
}

func TestChangePassword_OK(t *testing.T) {
	dir := t.TempDir()
	globals.SetConfig(&configuration.Config{
		Security: configuration.SecurityConfig{
			Authentication: configuration.AuthenticationConfig{
				Enabled: true,
				Mode:    "basic",
			},
		},
		Store: configuration.StoreConfig{Folder: dir},
	})

	security.CreateBasicUser(&security.BasicUser{Username: "alice", Password: "old", Role: security.RoleUser})

	restoreIn := mockStdin("alice\n")
	defer restoreIn()

	cmd.ReadPassword = func(int) ([]byte, error) { return []byte("newpass"), nil }

	buf, restore := captureCmdOutput()
	defer restore()

	cmd.ChangePassword()

	clean := stripANSI(buf.Bytes())
	if !bytes.Contains(clean, []byte("updated successfully")) {
		t.Fatalf("expected success message, got: %s", string(clean))
	}
}
