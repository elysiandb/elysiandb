package cmd

import (
	"github.com/taymour/elysiandb/internal/globals"
)

const CreateUserCommand = "create-user"
const DeleteUserCommand = "delete-user"
const ServerCommand = "server"
const HelpCommand = "help"

func GetAvailableCommands() map[string]string {
	return map[string]string{
		ServerCommand:     "Start ElysianDB server",
		CreateUserCommand: "Create a new user (needs security.authentication.mode = basic or user)",
		DeleteUserCommand: "Delete an existing user (needs security.authentication.mode = basic or user)",
		HelpCommand:       "List available commands",
	}
}

func GetHandlers() map[string]func() {
	return map[string]func(){
		CreateUserCommand: CreateUser,
		DeleteUserCommand: DeleteUser,
		ServerCommand:     StartServer,
		HelpCommand:       PrintHelp,
	}
}

func PrintHelp() {
	commands := GetAvailableCommands()

	for name, description := range commands {
		Printf("  %s%s%s  %s\n", globals.Bold, name, globals.Reset, description)
	}
}
