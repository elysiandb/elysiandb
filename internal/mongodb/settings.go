package mongodb

import (
	"context"
	"errors"
	"time"

	"github.com/taymour/elysiandb/internal/globals"
	"go.mongodb.org/mongo-driver/v2/bson"
	"go.mongodb.org/mongo-driver/v2/mongo"
	"go.mongodb.org/mongo-driver/v2/mongo/options"
)

const SettingsCollection = "_elysiandb_settings"

func IsReservedCollection(entity string) bool {
	return entity == SettingsCollection
}

type setting struct {
	Value string `bson:"value"`
}

func GetOrCreateSetting(name string, create func() (string, error)) (string, error) {
	ctx, cancel := context.WithTimeout(context.Background(), 10*time.Second)
	defer cancel()

	collection := globals.MongoDB.Collection(SettingsCollection)

	var existing setting
	err := collection.FindOne(ctx, bson.M{"_id": name}).Decode(&existing)
	if err == nil && existing.Value != "" {
		return existing.Value, nil
	}
	if err != nil && !errors.Is(err, mongo.ErrNoDocuments) {
		return "", err
	}

	value, err := create()
	if err != nil {
		return "", err
	}

	var stored setting
	err = collection.FindOneAndUpdate(
		ctx,
		bson.M{"_id": name},
		bson.M{"$setOnInsert": bson.M{"value": value}},
		options.FindOneAndUpdate().SetUpsert(true).SetReturnDocument(options.After),
	).Decode(&stored)
	if mongo.IsDuplicateKeyError(err) {
		err = collection.FindOne(ctx, bson.M{"_id": name}).Decode(&stored)
	}
	if err != nil {
		return "", err
	}

	return stored.Value, nil
}
