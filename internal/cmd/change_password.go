package cmd

import (
	"fmt"
	"os"

	"github.com/taymour/elysiandb/internal/globals"
	"github.com/taymour/elysiandb/internal/security"
)

func ChangePassword() {
	cfg := globals.GetConfig()
	if !cfg.Security.Authentication.Enabled {
		Printf("%sAuthentication is disabled in the configuration.%s\n", globals.Gold, globals.Reset)
		return
	}

	if cfg.Security.Authentication.Mode != "basic" && cfg.Security.Authentication.Mode != "user" {
		Printf("%sCreate user command only supports basic and user authentication modes.%s\n", globals.Gold, globals.Reset)
		return
	}

	var username string
	fmt.Print("Enter username: ")
	fmt.Scanln(&username)

	Printf("Enter new password: ")
	bytePassword, err := ReadPassword(int(os.Stdin.Fd()))
	fmt.Println()
	if err != nil {
		Printf("%sFailed to read password: %v%s\n", globals.Red, err, globals.Reset)
		return
	}

	Printf("Re-enter new password: ")
	bytePasswordConfirm, err := ReadPassword(int(os.Stdin.Fd()))
	fmt.Println()
	if err != nil {
		Printf("%sFailed to read password confirmation: %v%s\n", globals.Red, err, globals.Reset)
		return
	}

	if string(bytePassword) != string(bytePasswordConfirm) {
		Printf("%sPasswords do not match.%s\n", globals.Red, globals.Reset)
		return
	}

	err = security.ChangeUserPassword(username, string(bytePassword))
	if err != nil {
		Printf("%sFailed to update password: %v%s\n", globals.Red, err, globals.Reset)
		return
	}

	Printf("%sPassword updated successfully for user '%s'.%s\n", globals.Gold, username, globals.Reset)
}
