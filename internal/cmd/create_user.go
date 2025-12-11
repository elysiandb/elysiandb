package cmd

import (
	"fmt"
	"os"

	"github.com/taymour/elysiandb/internal/globals"
	"github.com/taymour/elysiandb/internal/security"
)

func CreateUser() {
	cfg := globals.GetConfig()
	if !cfg.Security.Authentication.Enabled {
		Printf("%sAuthentication is disabled in the configuration.%s\n", globals.Gold, globals.Reset)
		return
	}

	if cfg.Security.Authentication.Mode != "basic" && cfg.Security.Authentication.Mode != "user" {
		Printf("%sCreate user command only supports basic and user authentication modes.%s\n", globals.Gold, globals.Reset)
		return
	}

	newUser, err := getUserInput()
	if err != nil {
		fmt.Println(err)
		return
	}

	err = security.CreateBasicUser(newUser)
	if err != nil {
		Printf("%sFailed to create user: %v%s\n", globals.Red, err, globals.Reset)
		return
	}

	Printf("%sUser '%s' created successfully.%s\n", globals.Gold, newUser.Username, globals.Reset)
}

func getUserInput() (*security.BasicUser, error) {
	var username string
	var roleInput int

	fmt.Print("Enter new username: ")
	fmt.Scanln(&username)

	fmt.Println("Select role:")
	fmt.Println("1) admin")
	fmt.Println("2) user")
	fmt.Print("Choice: ")
	fmt.Scanln(&roleInput)

	var role security.Role
	if roleInput == 1 {
		role = security.RoleAdmin
	} else {
		role = security.RoleUser
	}

	Printf("Enter new password: ")
	bytePassword, err := ReadPassword(int(os.Stdin.Fd()))
	fmt.Println()
	if err != nil {
		return nil, fmt.Errorf("%sFailed to read password: %v%s", globals.Red, err, globals.Reset)
	}

	Printf("Re-enter new password: ")
	bytePasswordConfirm, err := ReadPassword(int(os.Stdin.Fd()))
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
		Role:     role,
	}, nil
}
