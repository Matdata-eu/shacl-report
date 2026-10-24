# Rule coverage demo sets

Each folder has a `shapes.ttl` and a `data.ttl`. Run one with the coverage
printout:

```sh
shacl-report --data examples/coverage/people/data.ttl \
             --shapes examples/coverage/people/shapes.ttl --coverage
```

or open it in the viewer (Coverage tab) with `--view`, or write a standalone
page with `--html coverage.html`.

| Set | Resources covered | Values checked | What it shows |
| --- | --- | --- | --- |
| [`webshop`](webshop) | 100 % | 100 % | A fully covered catalogue. `data-new-field.ttl` adds a property without a rule and fails `--coverage-min 100`. |
| [`paths`](paths) | 88.2 % | 75.0 % | Every kind of property path and every way a shape reaches a resource it does not target. |
| [`people`](people) | 60.1 % | 58.3 % | A typical data set with gaps: subclasses, untyped blank nodes, a typo in a path, a dead shape, a closed shape, a SPARQL constraint. |
| [`library`](library) | 57.9 % | 47.5 % | An ontology mixed into the data, multi-typed resources, every kind of target, a deactivated shape and a custom constraint component. |

## webshop: coverage in CI

```sh
# passes: every value is checked
shacl-report --data examples/coverage/webshop/data.ttl \
             --shapes examples/coverage/webshop/shapes.ttl --coverage-min 100

# fails with exit code 1: shop:giftWrap has no rule
shacl-report --data examples/coverage/webshop/data-new-field.ttl \
             --shapes examples/coverage/webshop/shapes.ttl --coverage-min 100
```

The offers are blank nodes that only `sh:node shop:OfferShape` reaches, so
half of the resources count as *reached* rather than *targeted*.

## paths: how paths cover triples

- `( ex:memberOf ex:site )`: a sequence covers the employee's `ex:memberOf`
  triple and the department's `ex:site` triple.
- `[ sh:alternativePath ( ex:phone ex:mobile ) ]`, `[ sh:zeroOrOnePath ex:deputy ]`
  and `[ sh:oneOrMorePath ex:partOf ]`.
- `[ sh:inversePath ex:heads ]` on the department covers the employees'
  `ex:heads` triples.
- `sh:node ex:EmployeeShape` on `ex:reportsTo` is recursive.
- `sh:xone` of two property shapes on a node shape (category: logic).
- The milestones (blank nodes) are reached through a nested `sh:property` on a
  property shape; their `ex:note` is not checked.
- The project steps are an RDF list. The list cells are left out; the steps
  are reached through `sh:node ex:StepShape`, but their `ex:effort` is not
  checked.
- `o:intranet` is an `ex:Project` without `ex:budget`, so
  `sh:targetSubjectsOf ex:budget` misses it.
- Gaps to find: `ex:hobby`, `ex:carPlate`, `ex:costCentre`, `ex:owner`, the
  company's properties, and the intranet milestone (its project is not
  targeted).

## people: a realistic set with gaps

- `ex:Employee rdfs:subClassOf ex:Person` is in the data: employees are
  checked by the person shape too, and appear under Person in the class tree.
- `ex:Car rdfs:subClassOf ex:Vehicle` is only in the shapes graph: the 12 cars
  are not checked, and a hint says so.
- `ex:Invoice` has no shape at all.
- Contact points are untyped blank nodes, grouped as
  *ex:Person › ex:contactPoint* and *ex:Employee › ex:contactPoint*.
- `sh:path ex:adress` is a typo; the report suggests `ex:address`.
- `ex:TruckShape` targets a class with no instances.
- The vehicle shape is closed: the vehicles are covered, their triples only
  where a property shape checks them.
- A `sh:sparql` constraint on the employee shape checks `ex:salary`.

`data.ttl` is made by `python3 generate.py > data.ttl` with a fixed seed;
change the counts in the script to try a larger set.

## library: schema in the data, every kind of target

- The ontology (`owl:Class`, `rdfs:subClassOf`, property declarations) is in
  `data.ttl` and left out of the counts (*Ontology and shapes in the data*).
- Books are both `lib:Book` and `schema:CreativeWork` and count in both.
- `lib:Ebook rdfs:subClassOf lib:Book` is in the data, so ebooks are books.
- Targets: `sh:targetClass`, `sh:targetSubjectsOf lib:lender` (the loans have
  no type), `sh:targetObjectsOf lib:item`, `sh:targetNode`. `l:loan3` has no
  lender, so it is not a loan for SHACL.
- `lib:MemberShape` is deactivated: members are not covered.
- `lib:OverdueShape` uses a SPARQL-based target, which is not evaluated.
- `lib:isbnCheck` is a parameter of a custom SPARQL constraint component.
- Try `--coverage-ignore-annotations` to leave out `rdfs:label`,
  `skos:altLabel` and `skos:definition`.
