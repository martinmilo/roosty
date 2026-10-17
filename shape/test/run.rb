# Entry point for the suite. bin/verify in the work folder calls it; the shaper runs it as
# ruby test/run.rb. Both build the program first when ../work/bin/build exists.
#
#   ruby test/run.rb            the work in hand: every test file this branch adds or changes
#                               since it left the trunk, committed or not
#   ruby test/run.rb --all      every test file
#   ruby test/run.rb -n /redeem/ -v      any Minitest option passes through
#
# The shaper commits each piece of work on its own branch and the worker continues on it,
# so the branch's own test files are the list, and nobody maintains one by hand.
require "open3"
require_relative "support/target"

TRUNK = "main" # the branch that pieces of work start from

abort "The test kit needs Ruby 3.0 or later; this is #{RUBY_VERSION}." if Gem::Version.new(RUBY_VERSION) < Gem::Version.new("3.0")
git_version = `git --version 2>/dev/null`[/\d+(\.\d+)+/] or abort "The test kit needs Git 2.28 or later, and git is not on the PATH."
abort "The test kit needs Git 2.28 or later; this is #{git_version}." if Gem::Version.new(git_version.split(".").first(3).join(".")) < Gem::Version.new("2.28")

shape = File.expand_path("..", __dir__)
Dir.chdir(shape)

def trunk
  [TRUNK, "origin/#{TRUNK}"].find { |name| system("git", "rev-parse", "--verify", "--quiet", "#{name}^{commit}", out: File::NULL, err: File::NULL) }
end

def test_files_on_this_branch
  base, status = Open3.capture2("git", "merge-base", trunk.to_s, "HEAD", err: File::NULL)
  abort "Cannot tell where this branch left #{TRUNK}, so the work in hand is unknown. " \
        "Run inside a Git repository that has a #{TRUNK} branch (local or origin/#{TRUNK}), or use --all." unless status.success?
  changed, = Open3.capture2("git", "diff", "--name-only", "--relative", "--diff-filter=AMR", base.strip, "--", "test")
  untracked, = Open3.capture2("git", "ls-files", "--others", "--exclude-standard", "--", "test")
  (changed.lines + untracked.lines).map(&:chomp).grep(/_test\.rb\z/).select { |file| File.file?(file) }.uniq
end

all = ARGV.delete("--all")
files = all ? Dir["test/**/*_test.rb"] : test_files_on_this_branch
abort(all ? "No test files under test/." : "No test file is added or changed on this branch since it left #{TRUNK}; use --all for the whole suite.") if files.empty?

# The build runs quietly: its output names source files, which the shaper must not see.
# When it fails, the output is the reason, and it is shown.
build = Target.path("bin/build")
if File.executable?(build)
  output, status = Open3.capture2e(build, chdir: Target.root)
  abort "bin/build in the work folder failed, so the tests did not run. It printed:\n#{output}" unless status.success?
end

require_relative "test_helper"
files.sort.each { |file| require File.join(shape, file) }

# A run in which no test executed must not look like a pass.
ran = 0
Minitest::Test.prepend(Module.new { define_method(:before_setup) { ran += 1; super() } })
Minitest.after_run do
  next unless ran.zero?
  $stdout.flush
  abort "\nNo tests ran, so this is not a pass. Check the -n filter you passed."
end
