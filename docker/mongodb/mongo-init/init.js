const elysianDb = db.getSiblingDB("elysiandb")

elysianDb.getCollection("_elysiandb_init").insertOne({
  createdAt: new Date(),
  source: "docker-init"
})

elysianDb.createUser({
  user: "elysian",
  pwd: "elysian",
  roles: [
    { role: "readWrite", db: "elysiandb" }
  ]
})
