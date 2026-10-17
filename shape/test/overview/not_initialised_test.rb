class NotInitialisedTest < AcceptanceTest
  test "without .roosty/project.yml the overview says to run roosty init" do
    write_file "README.md", "# Atelier\n"

    overview

    assert_exit 2
    assert_stderr "roosty init"
    assert_nothing_written
  end
end
