#!/bin/sh
# prints one http row for each file in app/controllers, a path it looks up from the
# directory it is started in. Started anywhere but the repository root, it exits 1.
if [ ! -d app/controllers ]; then
  echo "started in $(pwd), which has no app/controllers" >&2
  exit 1
fi
echo "# covers: http"
for file in app/controllers/*_controller.rb; do
  printf 'http\tGET /%s\t%s\n' "$(basename "$file" _controller.rb)" "$file"
done
