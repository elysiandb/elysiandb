package mongodb

import (
	"context"
	"io"

	"github.com/taymour/elysiandb/internal/globals"
	"go.mongodb.org/mongo-driver/v2/bson"
)

const restoreBatchSize = 1000

func IterateEntities(entity string, fn func(data map[string]any) error) error {
	ctx := context.Background()

	cursor, err := globals.MongoDB.Collection(entity).Find(ctx, bson.M{})
	if err != nil {
		return err
	}

	defer cursor.Close(ctx)

	for cursor.Next(ctx) {
		raw := map[string]any{}
		if err := cursor.Decode(&raw); err != nil {
			return err
		}

		if err := fn(NormalizeMongoDocument(raw)); err != nil {
			return err
		}
	}

	return cursor.Err()
}

func RestoreEntities(entity string, next func() (map[string]any, error)) (int, error) {
	ctx := context.Background()
	collection := globals.MongoDB.Collection(entity)

	AddEntityType(entity)

	count := 0
	batch := make([]any, 0, restoreBatchSize)
	flush := func() error {
		if len(batch) == 0 {
			return nil
		}

		_, err := collection.InsertMany(ctx, batch)
		batch = batch[:0]

		return err
	}

	for {
		data, err := next()
		if err == io.EOF {
			break
		}

		if err != nil {
			return count, err
		}

		batch = append(batch, ToMongoDocument(data))
		count++

		if len(batch) == restoreBatchSize {
			if err := flush(); err != nil {
				return count, err
			}
		}
	}

	return count, flush()
}

func DropAllCollections() error {
	ctx := context.Background()

	names, err := globals.MongoDB.ListCollectionNames(ctx, bson.M{})
	if err != nil {
		return err
	}

	for _, name := range names {
		if err := globals.MongoDB.Collection(name).Drop(ctx); err != nil {
			return err
		}
	}

	return nil
}
