package query

type Query struct {
	Entity string
	Offset int
	Limit  int
	Filter FilterNode
	Sorts  map[string]string
}

type FilterNode struct {
	Or   []FilterNode
	And  []FilterNode
	Leaf map[string]map[string]string
}
