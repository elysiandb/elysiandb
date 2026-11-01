package cmd

import (
	"fmt"
	"maps"
	"slices"

	"github.com/taymour/elysiandb/internal/globals"
	"golang.org/x/term"
)

var (
	Printf       = fmt.Printf
	ReadPassword = term.ReadPassword
)

const (
	CreateUserCommand     = "create-user"
	DeleteUserCommand     = "delete-user"
	ServerCommand         = "server"
	HelpCommand           = "help"
	ChangePasswordCommand = "change-password"
	ResetCommand          = "reset"
	BackupCommand         = "backup"
	RestoreCommand        = "restore"
)

func GetAvailableCommands() map[string]string {
	return map[string]string{
		ServerCommand:         "Start ElysianDB server",
		CreateUserCommand:     "Create a new user (needs security.authentication.mode = basic or user)",
		DeleteUserCommand:     "Delete an existing user (needs security.authentication.mode = basic or user)",
		ChangePasswordCommand: "Change password for an existing user (needs security.authentication.mode = basic or user)",
		ResetCommand:          "Reset the database by deleting all stored data and resets users (asks for yes, requires --force)",
		BackupCommand:         "Write a full backup archive: elysiandb backup [--output <folder>]",
		RestoreCommand:        "Replace all the data with a backup archive: elysiandb restore <file> [--force]",
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
		BackupCommand:         Backup,
		RestoreCommand:        Restore,
		HelpCommand:           PrintHelp,
	}
}

func PrintHelp() {
	commands := GetAvailableCommands()

	for _, name := range slices.Sorted(maps.Keys(commands)) {
		Printf("  %s%s%s  %s\n", globals.Bold, name, globals.Reset, commands[name])
	}
}
