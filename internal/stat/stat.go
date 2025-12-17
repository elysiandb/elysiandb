package stat

var Stats StatsContainer

func Init() {
	Stats = *NewStatsContainer()
}
