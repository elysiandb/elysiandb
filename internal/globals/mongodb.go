package globals

import "go.mongodb.org/mongo-driver/v2/mongo"

var (
	MongoDB     *mongo.Database
	MongoClient *mongo.Client
)
