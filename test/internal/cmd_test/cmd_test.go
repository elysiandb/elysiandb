package cmd_test

import (
	"bytes"
	"os"
	"testing"

	"github.com/taymour/elysiandb/internal/cmd"
	"github.com/taymour/elysiandb/internal/configuration"
	"github.com/taymour/elysiandb/internal/globals"
)

func TestDeleteUser_AuthDisabled(t *testing.T) {
	globals.SetConfig(&configuration.Config{})
	globals.GetConfig().Security.Authentication.Enabled = false

	r, w, _ := os.Pipe()
	stdout := os.Stdout
	os.Stdout = w

	cmd.DeleteUser()

	w.Close()
	os.Stdout = stdout
	var buf bytes.Buffer
	buf.ReadFrom(r)

	if !bytes.Contains(buf.Bytes(), []byte("Authentication is disabled")) {
		t.Fatalf("expected message about disabled authentication")
	}
}

func TestDeleteUser_WrongMode(t *testing.T) {
	globals.SetConfig(&configuration.Config{})
	globals.GetConfig().Security.Authentication.Enabled = true
	globals.GetConfig().Security.Authentication.Mode = "token"

	r, w, _ := os.Pipe()
	stdout := os.Stdout
	os.Stdout = w

	cmd.DeleteUser()

	w.Close()
	os.Stdout = stdout
	var buf bytes.Buffer
	buf.ReadFrom(r)

	if !bytes.Contains(buf.Bytes(), []byte("only supports basic")) {
		t.Fatalf("expected message about unsupported mode")
	}
}

func TestCreateUser_AuthDisabled(t *testing.T) {
	globals.SetConfig(&configuration.Config{})
	globals.GetConfig().Security.Authentication.Enabled = false

	r, w, _ := os.Pipe()
	stdout := os.Stdout
	os.Stdout = w

	cmd.CreateUser()

	w.Close()
	os.Stdout = stdout
	var buf bytes.Buffer
	buf.ReadFrom(r)

	if !bytes.Contains(buf.Bytes(), []byte("Authentication is disabled")) {
		t.Fatalf("expected message about disabled authentication")
	}
}

func TestCreateUser_WrongMode(t *testing.T) {
	globals.SetConfig(&configuration.Config{})
	globals.GetConfig().Security.Authentication.Enabled = true
	globals.GetConfig().Security.Authentication.Mode = "token"

	r, w, _ := os.Pipe()
	stdout := os.Stdout
	os.Stdout = w

	cmd.CreateUser()

	w.Close()
	os.Stdout = stdout
	var buf bytes.Buffer
	buf.ReadFrom(r)

	if !bytes.Contains(buf.Bytes(), []byte("only supports basic")) {
		t.Fatalf("expected message about unsupported mode")
	}
}
