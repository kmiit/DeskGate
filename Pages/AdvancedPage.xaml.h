#pragma once

#include "AdvancedPage.g.h"

namespace winrt::DeskGate::implementation
{
    struct AdvancedPage : AdvancedPageT<AdvancedPage>
    {
        AdvancedPage()
        {
            // Xaml objects should not call InitializeComponent during construction.
            // See https://github.com/microsoft/cppwinrt/tree/master/nuget#initializecomponent
        }

        int32_t MyProperty();
        void MyProperty(int32_t value);

        void myButton_Click(IInspectable const& sender, Microsoft::UI::Xaml::RoutedEventArgs const& args);
    };
}

namespace winrt::DeskGate::factory_implementation
{
    struct AdvancedPage : AdvancedPageT<AdvancedPage, implementation::AdvancedPage>
    {
    };
}
