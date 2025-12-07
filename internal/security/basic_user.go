package security

import (
	"crypto/rand"
	"crypto/sha256"
	"encoding/base64"
	"encoding/hex"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"os"
	"strings"

	"github.com/taymour/elysiandb/internal/globals"
	"github.com/valyala/fasthttp"
	"golang.org/x/crypto/bcrypt"
)

const (
	UsersFilename    = "users.json"
	KeyFilename      = "users.key"
	SessionsFilename = "sessions.json"
)

type Role string

const (
	RoleAdmin Role = "admin"
	RoleUser  Role = "user"
)

type BasicUser struct {
	Username string
	Password string
	Role     Role
}

func (u *BasicUser) ToHasedUser() (*BasicHashedcUser, error) {
	key, err := CreateKeyFileOrGetKey()
	if err != nil {
		return nil, err
	}

	sum := sha256.Sum256([]byte(u.Password + key))
	hashedPassword, err := bcrypt.GenerateFromPassword(sum[:], bcrypt.DefaultCost)
	if err != nil {
		return nil, err
	}

	role := u.Role
	if role == "" {
		role = RoleUser
	}

	return &BasicHashedcUser{
		Username: u.Username,
		Password: string(hashedPassword),
		Role:     role,
	}, nil
}

type BasicHashedcUser struct {
	Username string `json:"username"`
	Password string `json:"password"`
	Role     Role   `json:"role"`
}

type UsersFile struct {
	Users []BasicHashedcUser `json:"users"`
}

func CreateBasicUser(user *BasicUser) error {
	hashedUser, err := user.ToHasedUser()
	if err != nil {
		return err
	}

	err = AddUserToFile(hashedUser)
	if err != nil {
		return err
	}

	return nil
}

func DeleteBasicUser(username string) error {
	usersFile, err := LoadUsersFromFile()
	if err != nil {
		if errors.Is(err, os.ErrNotExist) {
			return fmt.Errorf("no users exist yet")
		}
		return err
	}

	var updatedUsers []BasicHashedcUser
	found := false
	for _, user := range usersFile.Users {
		if user.Username != username {
			updatedUsers = append(updatedUsers, user)
		} else {
			found = true
		}
	}

	if !found {
		return fmt.Errorf("user '%s' not found", username)
	}

	usersFile.Users = updatedUsers

	cfg := globals.GetConfig()
	file, err := os.OpenFile(fmt.Sprintf("%s/%s", cfg.Store.Folder, UsersFilename), os.O_RDWR|os.O_TRUNC, 0644)
	if err != nil {
		return err
	}
	defer file.Close()

	encoder := json.NewEncoder(file)
	if err := encoder.Encode(usersFile); err != nil {
		return err
	}

	return nil
}

func GenerateKey() (string, error) {
	b := make([]byte, 512)
	_, err := rand.Read(b)
	if err != nil {
		return "", fmt.Errorf("failed to generate key: %w", err)
	}

	return hex.EncodeToString(b), nil
}

func CreateKeyFileOrGetKey() (string, error) {
	cfg := globals.GetConfig()
	path := fmt.Sprintf("%s/%s", cfg.Store.Folder, KeyFilename)

	file, err := os.OpenFile(path, os.O_RDWR|os.O_CREATE, 0644)
	if err != nil {
		return "", err
	}
	defer file.Close()

	fileInfo, err := file.Stat()
	if err != nil {
		return "", err
	}

	if fileInfo.Size() == 0 {
		key, err := GenerateKey()
		if err != nil {
			return "", err
		}

		if _, err := file.WriteAt([]byte(key), 0); err != nil {
			return "", err
		}

		return key, nil
	}

	if _, err := file.Seek(0, 0); err != nil {
		return "", err
	}

	var key string
	_, err = fmt.Fscanf(file, "%s", &key)
	if err != nil {
		return "", err
	}

	return key, nil
}

func AddUserToFile(user *BasicHashedcUser) error {
	cfg := globals.GetConfig()
	file, err := os.OpenFile(fmt.Sprintf("%s/%s", cfg.Store.Folder, UsersFilename), os.O_RDWR|os.O_CREATE, 0644)
	if err != nil {
		return err
	}

	var users UsersFile
	fileInfo, err := file.Stat()
	if err != nil {
		return err
	}

	if fileInfo.Size() > 0 {
		decoder := json.NewDecoder(file)
		if err := decoder.Decode(&users); err != nil && err != io.EOF {
			return err
		}
	}

	users.Users = append(users.Users, *user)

	file.Truncate(0)
	file.Seek(0, 0)
	encoder := json.NewEncoder(file)
	if err := encoder.Encode(users); err != nil {
		return err
	}

	defer file.Close()

	return nil
}

func LoadUsersFromFile() (*UsersFile, error) {
	cfg := globals.GetConfig()
	file, err := os.Open(fmt.Sprintf("%s/%s", cfg.Store.Folder, UsersFilename))
	if err != nil {
		return nil, err
	}

	defer file.Close()

	var users UsersFile
	decoder := json.NewDecoder(file)
	if err := decoder.Decode(&users); err != nil {
		return nil, err
	}

	return &users, nil
}

func AuthenticateUser(username, password string) (*BasicHashedcUser, bool) {
	users, err := LoadUsersFromFile()
	if err != nil {
		return nil, false
	}

	key, err := CreateKeyFileOrGetKey()
	if err != nil {
		return nil, false
	}

	sum := sha256.Sum256([]byte(password + key))
	pass := sum[:]

	for i := range users.Users {
		u := users.Users[i]
		if u.Username == username {
			if bcrypt.CompareHashAndPassword([]byte(u.Password), pass) == nil {
				return &u, true
			}
			return nil, false
		}
	}

	return nil, false
}

var CheckBasicAuthentication = func(ctx *fasthttp.RequestCtx) bool {
	header := string(ctx.Request.Header.Peek("Authorization"))
	if header == "" || !strings.HasPrefix(header, "Basic ") {
		return false
	}

	payload, err := base64.StdEncoding.DecodeString(strings.TrimPrefix(header, "Basic "))
	if err != nil {
		return false
	}

	parts := strings.SplitN(string(payload), ":", 2)
	if len(parts) != 2 {
		return false
	}

	username := parts[0]
	password := parts[1]

	_, ok := AuthenticateUser(username, password)
	return ok
}
