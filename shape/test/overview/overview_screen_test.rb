class OverviewScreenTest < AcceptanceTest
  def setup = workspace("atelier") # the full fixture: description, two watch_out sentences, 3 processes, 7 entrypoints, a note on deploy

  test "the overview of Atelier is printed exactly as laid out and writes nothing" do
    overview

    assert_success
    assert_stdout_is expected("overview_atelier.txt")
    assert_nothing_written
  end
end
