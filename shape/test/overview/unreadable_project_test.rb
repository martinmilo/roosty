class UnreadableProjectTest < AcceptanceTest
  def setup = workspace("atelier")

  test "a project.yml that does not parse stops the overview and points at validate" do
    write_file ".roosty/project.yml", "name: [unclosed\n"

    overview

    assert_exit 2
    assert_stderr "project.yml"
    assert_stderr "roosty validate"
    assert_nothing_written
  end
end
