package cmd_test

import (
	"bytes"
	"errors"
	"fmt"
	"os"
	"path/filepath"
	"strings"
	"testing"

	api_storage "github.com/taymour/elysiandb/internal/api"
	"github.com/taymour/elysiandb/internal/cmd"
	"github.com/taymour/elysiandb/internal/configuration"
	"github.com/taymour/elysiandb/internal/globals"
	"github.com/taymour/elysiandb/internal/security"
	"github.com/taymour/elysiandb/internal/storage"
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

func initCmdStore(t *testing.T, cfg *configuration.Config) {
	t.Helper()
	if cfg.Store.Folder == "" {
		cfg.Store.Folder = t.TempDir()
	}
	if cfg.Store.Shards == 0 {
		cfg.Store.Shards = 8
	}
	globals.SetConfig(cfg)
	storage.LoadDB()
	storage.LoadJsonDB()
}

func TestDeleteUser_AuthDisabled(t *testing.T) {
	cfg := &configuration.Config{
		Security: configuration.SecurityConfig{
			Authentication: configuration.AuthenticationConfig{
				Enabled: false,
			},
		},
	}
	initCmdStore(t, cfg)

	buf, restore := captureCmdOutput()
	defer restore()

	cmd.DeleteUser()

	clean := stripANSI(buf.Bytes())
	if !bytes.Contains(clean, []byte("Authentication is disabled")) {
		t.Fatalf("expected message, got: %s", string(clean))
	}
}

func TestDeleteUser_WrongMode(t *testing.T) {
	cfg := &configuration.Config{
		Security: configuration.SecurityConfig{
			Authentication: configuration.AuthenticationConfig{
				Enabled: true,
				Mode:    "token",
			},
		},
	}
	initCmdStore(t, cfg)

	buf, restore := captureCmdOutput()
	defer restore()

	cmd.DeleteUser()

	clean := stripANSI(buf.Bytes())
	if !bytes.Contains(clean, []byte("only supports basic")) {
		t.Fatalf("expected message, got: %s", string(clean))
	}
}

func TestCreateUser_AuthDisabled(t *testing.T) {
	cfg := &configuration.Config{
		Security: configuration.SecurityConfig{
			Authentication: configuration.AuthenticationConfig{
				Enabled: false,
			},
		},
	}
	initCmdStore(t, cfg)

	buf, restore := captureCmdOutput()
	defer restore()

	cmd.CreateUser()

	clean := stripANSI(buf.Bytes())
	if !bytes.Contains(clean, []byte("Authentication is disabled")) {
		t.Fatalf("expected message, got: %s", string(clean))
	}
}

func TestCreateUser_WrongMode(t *testing.T) {
	cfg := &configuration.Config{
		Security: configuration.SecurityConfig{
			Authentication: configuration.AuthenticationConfig{
				Enabled: true,
				Mode:    "token",
			},
		},
	}
	initCmdStore(t, cfg)

	buf, restore := captureCmdOutput()
	defer restore()

	cmd.CreateUser()

	clean := stripANSI(buf.Bytes())
	if !bytes.Contains(clean, []byte("only supports basic")) {
		t.Fatalf("expected message, got: %s", string(clean))
	}
}

func TestDeleteUser_OK(t *testing.T) {
	cfg := &configuration.Config{
		Security: configuration.SecurityConfig{
			Authentication: configuration.AuthenticationConfig{
				Enabled: true,
				Mode:    "basic",
			},
		},
	}
	initCmdStore(t, cfg)

	if err := security.CreateBasicUser(&security.BasicUser{Username: "john", Password: "x", Role: security.RoleUser}); err != nil {
		t.Fatalf("failed to create user: %v", err)
	}

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
	cfg := &configuration.Config{
		Security: configuration.SecurityConfig{
			Authentication: configuration.AuthenticationConfig{
				Enabled: true,
				Mode:    "basic",
			},
		},
	}
	initCmdStore(t, cfg)

	restoreIn := mockStdin("ghost\n")
	defer restoreIn()

	buf, restore := captureCmdOutput()
	defer restore()

	cmd.DeleteUser()

	clean := stripANSI(buf.Bytes())
	if !bytes.Contains(clean, []byte("deleted successfully")) {
		t.Fatalf("expected deletion message even for unknown user, got: %s", string(clean))
	}
}

func TestCreateUser_OK(t *testing.T) {
	cfg := &configuration.Config{
		Security: configuration.SecurityConfig{
			Authentication: configuration.AuthenticationConfig{
				Enabled: true,
				Mode:    "basic",
			},
		},
	}
	initCmdStore(t, cfg)

	restoreIn := mockStdin("alice\n1\n")
	defer restoreIn()

	origReadPassword := cmd.ReadPassword
	cmd.ReadPassword = func(int) ([]byte, error) { return []byte("secret"), nil }
	defer func() { cmd.ReadPassword = origReadPassword }()

	buf, restore := captureCmdOutput()
	defer restore()

	cmd.CreateUser()

	clean := stripANSI(buf.Bytes())
	if !bytes.Contains(clean, []byte("created successfully")) {
		t.Fatalf("expected success message, got: %s", string(clean))
	}
}

func TestCreateUser_PasswordMismatch(t *testing.T) {
	cfg := &configuration.Config{
		Security: configuration.SecurityConfig{
			Authentication: configuration.AuthenticationConfig{
				Enabled: true,
				Mode:    "basic",
			},
		},
	}
	initCmdStore(t, cfg)

	input := "bob\n2\n"
	restoreIn := mockStdin(input)
	defer restoreIn()

	call := 0
	origReadPassword := cmd.ReadPassword
	cmd.ReadPassword = func(int) ([]byte, error) {
		if call == 0 {
			call++
			return []byte("pass1"), nil
		}
		return []byte("pass2"), nil
	}
	defer func() { cmd.ReadPassword = origReadPassword }()

	r, w, _ := os.Pipe()
	origStdout := os.Stdout
	os.Stdout = w

	cmd.CreateUser()

	w.Close()
	os.Stdout = origStdout
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
		cmds[cmd.HelpCommand] == "" ||
		cmds[cmd.ChangePasswordCommand] == "" ||
		cmds[cmd.ResetCommand] == "" ||
		cmds[cmd.BackupCommand] == "" ||
		cmds[cmd.RestoreCommand] == "" {
		t.Fatalf("expected all commands to be present")
	}
}

func TestGetHandlers(t *testing.T) {
	h := cmd.GetHandlers()

	if h[cmd.ServerCommand] == nil ||
		h[cmd.CreateUserCommand] == nil ||
		h[cmd.DeleteUserCommand] == nil ||
		h[cmd.HelpCommand] == nil ||
		h[cmd.ChangePasswordCommand] == nil ||
		h[cmd.ResetCommand] == nil ||
		h[cmd.BackupCommand] == nil ||
		h[cmd.RestoreCommand] == nil {
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
		cmd.ChangePasswordCommand,
		cmd.ResetCommand,
		cmd.BackupCommand,
		cmd.RestoreCommand,
	}

	for _, e := range expected {
		if !bytes.Contains(out, []byte(e)) {
			t.Fatalf("expected help output to contain %s, got: %s", e, string(out))
		}
	}
}

func TestChangePassword_AuthDisabled(t *testing.T) {
	cfg := &configuration.Config{
		Security: configuration.SecurityConfig{
			Authentication: configuration.AuthenticationConfig{
				Enabled: false,
			},
		},
	}
	initCmdStore(t, cfg)

	buf, restore := captureCmdOutput()
	defer restore()

	cmd.ChangePassword()

	clean := stripANSI(buf.Bytes())
	if !bytes.Contains(clean, []byte("Authentication is disabled")) {
		t.Fatalf("expected message, got: %s", string(clean))
	}
}

func TestChangePassword_WrongMode(t *testing.T) {
	cfg := &configuration.Config{
		Security: configuration.SecurityConfig{
			Authentication: configuration.AuthenticationConfig{
				Enabled: true,
				Mode:    "token",
			},
		},
	}
	initCmdStore(t, cfg)

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

	calls := 0
	origReadPassword := cmd.ReadPassword
	cmd.ReadPassword = func(int) ([]byte, error) {
		calls++
		return []byte("whatever"), nil
	}
	defer func() { cmd.ReadPassword = origReadPassword }()

	buf, restore := captureCmdOutput()
	defer restore()

	cmd.ChangePassword()

	clean := stripANSI(buf.Bytes())

	if !bytes.Contains(clean, []byte("Failed to update password")) &&
		!bytes.Contains(clean, []byte("not found")) {
		t.Fatalf("expected not found error, got: %s", string(clean))
	}

	if calls != 2 {
		t.Fatalf("expected 2 password prompts, got %d", calls)
	}
}

func TestChangePassword_Mismatch(t *testing.T) {
	cfg := &configuration.Config{
		Security: configuration.SecurityConfig{
			Authentication: configuration.AuthenticationConfig{
				Enabled: true,
				Mode:    "basic",
			},
		},
	}
	initCmdStore(t, cfg)

	if err := security.CreateBasicUser(&security.BasicUser{Username: "bob", Password: "x", Role: security.RoleUser}); err != nil {
		t.Fatalf("failed to create user: %v", err)
	}

	restoreIn := mockStdin("bob\n")
	defer restoreIn()

	call := 0
	origReadPassword := cmd.ReadPassword
	cmd.ReadPassword = func(int) ([]byte, error) {
		if call == 0 {
			call++
			return []byte("pass1"), nil
		}
		return []byte("pass2"), nil
	}
	defer func() { cmd.ReadPassword = origReadPassword }()

	_, w, _ := os.Pipe()
	origOut := os.Stdout
	os.Stdout = w
	var buf bytes.Buffer
	origPrintf := cmd.Printf
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
	cfg := &configuration.Config{
		Security: configuration.SecurityConfig{
			Authentication: configuration.AuthenticationConfig{
				Enabled: true,
				Mode:    "basic",
			},
		},
	}
	initCmdStore(t, cfg)

	if err := security.CreateBasicUser(&security.BasicUser{Username: "alice", Password: "old", Role: security.RoleUser}); err != nil {
		t.Fatalf("failed to create user: %v", err)
	}

	restoreIn := mockStdin("alice\n")
	defer restoreIn()

	origReadPassword := cmd.ReadPassword
	cmd.ReadPassword = func(int) ([]byte, error) { return []byte("newpass"), nil }
	defer func() { cmd.ReadPassword = origReadPassword }()

	buf, restore := captureCmdOutput()
	defer restore()

	cmd.ChangePassword()

	clean := stripANSI(buf.Bytes())
	if !bytes.Contains(clean, []byte("updated successfully")) {
		t.Fatalf("expected success message, got: %s", string(clean))
	}
}

func TestResetAll_AbortOnNo(t *testing.T) {
	dir := t.TempDir()
	globals.SetConfig(&configuration.Config{
		Store: configuration.StoreConfig{Folder: dir},
	})

	restoreIn := mockStdin("no\n")
	defer restoreIn()

	var buf bytes.Buffer
	origPrintf := cmd.Printf
	cmd.Printf = func(format string, a ...interface{}) (int, error) {
		return buf.WriteString(fmt.Sprintf(format, a...))
	}
	defer func() { cmd.Printf = origPrintf }()

	cmd.ResetAll()

	out := buf.String()

	if !bytes.Contains([]byte(out), []byte("Reset aborted.")) {
		t.Fatalf("expected abort message, got: %s", out)
	}

	if _, err := os.Stat(dir); err != nil {
		t.Fatalf("expected directory to still exist, got: %v", err)
	}
}

func TestResetAll_WithForceDeletesFolder(t *testing.T) {
	dir := t.TempDir()
	globals.SetConfig(&configuration.Config{
		Store: configuration.StoreConfig{Folder: dir},
	})

	origArgs := os.Args
	os.Args = []string{"elysiandb", cmd.ResetCommand, "--force"}
	defer func() { os.Args = origArgs }()

	restoreIn := mockStdin("yes\n")
	defer restoreIn()

	r, w, _ := os.Pipe()
	origOut := os.Stdout
	os.Stdout = w

	cmd.ResetAll()

	w.Close()
	os.Stdout = origOut
	var buf bytes.Buffer
	buf.ReadFrom(r)

	if _, err := os.Stat(dir); !os.IsNotExist(err) {
		t.Fatalf("expected directory to be removed, got: %v", err)
	}
}

func TestDeleteUser_DefaultAdminIsKept(t *testing.T) {
	cfg := &configuration.Config{
		Security: configuration.SecurityConfig{
			Authentication: configuration.AuthenticationConfig{
				Enabled: true,
				Mode:    "user",
			},
		},
	}
	initCmdStore(t, cfg)

	if err := security.CreateBasicUser(&security.BasicUser{Username: security.DefaultAdminUsername, Password: "x", Role: security.RoleAdmin}); err != nil {
		t.Fatalf("failed to create admin: %v", err)
	}

	restoreIn := mockStdin("admin\n")
	defer restoreIn()

	buf, restore := captureCmdOutput()
	defer restore()

	cmd.DeleteUser()

	clean := stripANSI(buf.Bytes())
	if !bytes.Contains(clean, []byte("was not deleted")) || bytes.Contains(clean, []byte("deleted successfully")) {
		t.Fatalf("expected the deletion to be refused, got: %s", string(clean))
	}

	if _, err := security.GetBasicUserByUsername(security.DefaultAdminUsername); err != nil {
		t.Fatalf("the default admin must still exist: %v", err)
	}
}

func TestPrintHelpListsCommandsInOrder(t *testing.T) {
	buf, restore := captureCmdOutput()
	defer restore()

	cmd.PrintHelp()

	out := string(stripANSI(buf.Bytes()))
	if strings.Index(out, cmd.BackupCommand) > strings.Index(out, cmd.RestoreCommand) || strings.Index(out, cmd.ChangePasswordCommand) > strings.Index(out, cmd.ServerCommand) {
		t.Fatalf("expected the commands in alphabetical order, got: %s", out)
	}
}

func TestRunBackupWritesAnArchiveInTheOutputFolder(t *testing.T) {
	initCmdStore(t, &configuration.Config{})
	api_storage.WriteEntity("book", map[string]any{"id": "b1"})
	output := t.TempDir()

	for _, args := range [][]string{{"--output", output}, {"--output=" + output}, {"-o", output}} {
		path, err := cmd.RunBackup(args)
		if err != nil {
			t.Fatalf("backup %v failed: %v", args, err)
		}

		if filepath.Dir(path) != output || !strings.HasPrefix(filepath.Base(path), "elysiandb-backup-") || !strings.HasSuffix(path, ".tar.gz") {
			t.Fatalf("unexpected backup path %s", path)
		}

		os.Remove(path)
	}
}

func TestRunBackupRejectsInvalidArguments(t *testing.T) {
	initCmdStore(t, &configuration.Config{})
	file := filepath.Join(t.TempDir(), "file")
	os.WriteFile(file, []byte("x"), 0o600)

	for _, args := range [][]string{{"--output"}, {"--unknown"}, {"--output", filepath.Join(t.TempDir(), "missing")}, {"--output", file}} {
		if _, err := cmd.RunBackup(args); err == nil {
			t.Fatalf("expected backup %v to fail", args)
		}
	}
}

func TestRunRestoreAsksForConfirmation(t *testing.T) {
	initCmdStore(t, &configuration.Config{})
	api_storage.WriteEntity("book", map[string]any{"id": "b1"})
	path, err := cmd.RunBackup([]string{"--output", t.TempDir()})
	if err != nil {
		t.Fatalf("backup failed: %v", err)
	}

	initCmdStore(t, &configuration.Config{})
	api_storage.WriteEntity("keep", map[string]any{"id": "k1"})

	_, restoreOutput := captureCmdOutput()
	defer restoreOutput()

	if _, err := cmd.RunRestore([]string{path}, strings.NewReader("no\n")); !errors.Is(err, cmd.ErrRestoreAborted) {
		t.Fatalf("expected the restore to be aborted, got %v", err)
	}

	if api_storage.ReadEntityById("keep", "k1") == nil {
		t.Fatalf("an aborted restore must leave the data untouched")
	}

	report, err := cmd.RunRestore([]string{path}, strings.NewReader("yes\n"))
	if err != nil || report.Entities["book"] != 1 {
		t.Fatalf("expected the restore to succeed, got %+v (%v)", report, err)
	}

	if api_storage.ReadEntityById("keep", "k1") != nil || api_storage.ReadEntityById("book", "b1") == nil {
		t.Fatalf("expected the data of the archive only")
	}
}

func TestRunRestoreWithForceDoesNotAsk(t *testing.T) {
	initCmdStore(t, &configuration.Config{})
	api_storage.WriteEntity("book", map[string]any{"id": "b1"})
	path, _ := cmd.RunBackup([]string{"--output", t.TempDir()})

	initCmdStore(t, &configuration.Config{})
	if _, err := cmd.RunRestore([]string{"--force", path}, strings.NewReader("")); err != nil {
		t.Fatalf("expected the restore to succeed, got %v", err)
	}

	if api_storage.ReadEntityById("book", "b1") == nil {
		t.Fatalf("expected the document of the archive")
	}
}

func TestRunRestoreRefusesInvalidInput(t *testing.T) {
	initCmdStore(t, &configuration.Config{})
	invalid := filepath.Join(t.TempDir(), "invalid.tar.gz")
	os.WriteFile(invalid, []byte("not an archive"), 0o600)

	for _, args := range [][]string{{}, {"--force"}, {"a", "b"}, {"--unknown", invalid}, {filepath.Join(t.TempDir(), "missing")}, {invalid, "--force"}} {
		if _, err := cmd.RunRestore(args, strings.NewReader("yes\n")); err == nil || errors.Is(err, cmd.ErrRestoreAborted) {
			t.Fatalf("expected restore %v to fail, got %v", args, err)
		}
	}
}

func TestBackupAndRestoreHandlersExitOnError(t *testing.T) {
	initCmdStore(t, &configuration.Config{})

	buf, restoreOutput := captureCmdOutput()
	defer restoreOutput()

	code := 0
	origExit := cmd.Exit
	cmd.Exit = func(c int) { code = c }
	defer func() { cmd.Exit = origExit }()

	cmd.Restore()
	if code != 1 || !strings.Contains(string(stripANSI(buf.Bytes())), "Restore failed") {
		t.Fatalf("expected the restore handler to exit with 1, got %d: %s", code, buf.String())
	}
}
