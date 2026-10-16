# R's most loved libraries: what they teach Valvet, and what ports

Ranked from CRAN download counts (ggplot2 ~198M, rlang ~189M, magrittr
~164M, dplyr ~154M) plus community surveys (rstats, Posit lists,
actuarial top-10). rlang/magrittr top downloads as transitive
infrastructure, so this list favors packages users name directly.

## 1. ggplot2
Declarative graphics from the Grammar of Graphics. Most downloaded R
package; the reason many people learn R. Huge extension ecosystem.

## 2. dplyr
Readable data verbs (`filter`, `select`, `mutate`, `summarise`,
`arrange`, `group_by`) with lazy backends; `%>%` became R's idiom.

## 3. tidyr
Tidy-data reshaping (`pivot_longer`/`pivot_wider`, `separate`, `fill`,
`drop_na`).

## 4. data.table
In-memory speed with `DT[i, j, by]`; the friendly rival to the
tidyverse on performance.

## 5. stringr (over stringi)
Consistent string verbs ending base-R regex inconsistency.

## 6. lubridate
Human date parsing and arithmetic; makes hated date handling boring.

## 7. readr
Fast predictable flat-file parsing with column types and failure reports.

## 8. knitr + rmarkdown
Reproducible code-plus-prose reports; analysis made shareable.

## 9. Shiny
Reactive web apps in pure R, no JavaScript.

## 10. devtools (usethis, testthat)
Package scaffolding, `load_all`, testing workflow.

Just off the list: caret/tidymodels (ancestor of our `std::select`),
forcats (why factors are first-class), Rcpp (the two-language admission).

## Portability verdicts (Valvet constraints apply)

Fully or nearly portable: dplyr (~80% done; only unquoted NSE stays
out), tidyr (done; `unnest` writable today), stringr (engines exist),
lubridate core (full IANA tz is a data port, not a language problem),
readr core (no URLs/compressed/odd encodings), devtools workflow
(`expect_equal` assertions are trivial stdlib).

Partially portable: ggplot2 (string-column spec layer yes, `aes()` NSE
no), data.table (API subset yes in stdlib, index-driven speed parity
no), knitr (weave half yes as a CLI tool over the doc-test harvester,
render half no).

Not portable: Shiny (reactive server plus JS client contradicts the
ahead-of-time offline model).

Rule of thumb: data plus named functions ports; code-as-data (NSE,
closures, reactivity) does not. 6 of 10 fully, 3 partially, Shiny never.

## Proposed `tidy` track (zero core changes)

Finish dplyr/tidyr verb gaps, add the ggplot-style string-column spec
layer, port testthat assertions. All stdlib plus fixtures plus oracles.
