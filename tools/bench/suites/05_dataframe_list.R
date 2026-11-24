set.seed(42)

n <- 12000L
id <- seq_len(n)
labels <- paste0("item-", rep(1:4, length.out = n))
records <- data.frame(id = id, label = labels, value = id %% 53L,
                      stringsAsFactors = FALSE)
selected <- records[grepl("^item-[13]$", records$label), c("id", "label", "value")]
groups <- gsub("^item-", "group-", selected$label)
rows <- split(seq_len(nrow(selected)), groups)
scores <- lapply(rows, function(i) selected$id[i] + 2L * selected$value[i])
totals <- vapply(scores, sum, numeric(1))
counts <- vapply(scores, length, integer(1))

checks <- paste(names(totals), sprintf("%.0f", totals), counts, sep = "=")
cat("RESULT:", paste(checks, collapse = ","), "\n", sep = "")
