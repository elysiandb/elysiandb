package cmd

import (
	"fmt"

	"github.com/taymour/elysiandb/internal/globals"
	"github.com/taymour/elysiandb/internal/security"
)

func DeleteUser() {
	cfg := globals.GetConfig()
	if !cfg.Security.Authentication.Enabled {
		fmt.Printf("%sAuthentication is disabled in the configuration.%s\n", globals.Gold, globals.Reset)
		return
	}

	if cfg.Security.Authentication.Mode != "basic" {
		fmt.Printf("%sDelete user command only supports basic authentication mode.%s\n", globals.Gold, globals.Reset)
		return
	}

	username, err := getUsernameInput()
	if err != nil {
		fmt.Println(err)
		return
	}

	err = security.DeleteBasicUser(username)
	if err != nil {
		fmt.Printf("%sFailed to delete user: %v%s\n", globals.Red, err, globals.Reset)
		return
	}

	fmt.Printf("%sUser '%s' deleted successfully.%s\n", globals.Gold, username, globals.Reset)
}

func getUsernameInput() (string, error) {
	var username string
	fmt.Print("Enter username to delete: ")
	fmt.Scanln(&username)

	if username == "" {
		return "", fmt.Errorf("username cannot be empty")
	}

	return username, nil
}
