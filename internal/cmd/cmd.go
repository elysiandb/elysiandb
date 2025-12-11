package cmd

import (
	"fmt"

	"github.com/taymour/elysiandb/internal/globals"
	"golang.org/x/term"
)

var Printf = fmt.Printf
var ReadPassword = term.ReadPassword

const CreateUserCommand = "create-user"
const DeleteUserCommand = "delete-user"
const ServerCommand = "server"
const HelpCommand = "help"
const ChangePasswordCommand = "change-password"
const ResetCommand = "reset"

func GetAvailableCommands() map[string]string {
	return map[string]string{
		ServerCommand:         "Start ElysianDB server",
		CreateUserCommand:     "Create a new user (needs security.authentication.mode = basic or user)",
		DeleteUserCommand:     "Delete an existing user (needs security.authentication.mode = basic or user)",
		ChangePasswordCommand: "Change password for an existing user (needs security.authentication.mode = basic or user)",
		ResetCommand:          "Reset the database by deleting all stored data and resets users (requires --force flag)",
		HelpCommand:           "List available commands",
	}
}

func GetHandlers() map[string]func() {
	return map[string]func(){
		CreateUserCommand:     CreateUser,
		DeleteUserCommand:     DeleteUser,
		ServerCommand:         StartServer,
		ChangePasswordCommand: ChangePassword,
		ResetCommand:          ResetAll,
		HelpCommand:           PrintHelp,
	}
}

func PrintHelp() {
	commands := GetAvailableCommands()

	for name, description := range commands {
		Printf("  %s%s%s  %s\n", globals.Bold, name, globals.Reset, description)
	}
}
