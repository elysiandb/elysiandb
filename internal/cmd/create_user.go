package cmd

import (
	"fmt"
	"os"

	"github.com/taymour/elysiandb/internal/globals"
	"github.com/taymour/elysiandb/internal/security"
	"golang.org/x/term"
)

func CreateUser() {
	cfg := globals.GetConfig()
	if !cfg.Security.Authentication.Enabled {
		fmt.Printf("%sAuthentication is disabled in the configuration.%s\n", globals.Gold, globals.Reset)
		return
	}

	if cfg.Security.Authentication.Mode != "basic" {
		fmt.Printf("%sCreate user command only supports basic authentication mode.%s\n", globals.Gold, globals.Reset)
		return
	}

	newUser, err := getUserInput()
	if err != nil {
		fmt.Println(err)
		return
	}

	err = security.CreateBasicUser(newUser)
	if err != nil {
		fmt.Printf("%sFailed to create user: %v%s\n", globals.Red, err, globals.Reset)
		return
	}

	fmt.Printf("%sUser '%s' created successfully.%s\n", globals.Gold, newUser.Username, globals.Reset)
}

func getUserInput() (*security.BasicUser, error) {
	var username string
	fmt.Print("Enter new username: ")
	fmt.Scanln(&username)
	fmt.Print("Enter new password: ")
	bytePassword, err := term.ReadPassword(int(os.Stdin.Fd()))
	fmt.Println()
	if err != nil {
		return nil, fmt.Errorf("%sFailed to read password: %v%s", globals.Red, err, globals.Reset)
	}

	fmt.Print("Re-enter new password: ")
	bytePasswordConfirm, err := term.ReadPassword(int(os.Stdin.Fd()))
	fmt.Println()
	if err != nil {
		return nil, fmt.Errorf("%sFailed to read password confirmation: %v%s", globals.Red, err, globals.Reset)
	}

	if string(bytePassword) != string(bytePasswordConfirm) {
		return nil, fmt.Errorf("%sPasswords do not match.%s", globals.Red, globals.Reset)
	}

	return &security.BasicUser{
		Username: username,
		Password: string(bytePassword),
	}, nil
}
